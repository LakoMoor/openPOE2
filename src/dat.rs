//! Reader for `.datc64` tables: `u32` row count, fixed-width rows, an 8-byte
//! `0xBB` marker, then the variable-length section (strings, lists).

use anyhow::{bail, ensure, Context, Result};

use crate::schema::{TableSchema, Ty};

const MARKER: [u8; 8] = [0xbb; 8];
/// "No row" sentinel stored in foreign keys.
const NULL_ROW: u64 = 0xfefe_fefe_fefe_fefe;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f32),
    Str(String),
    /// Foreign key; `None` means no row.
    Ref(Option<u64>),
    List(Vec<Value>),
}

pub struct Table<'a> {
    data: &'a [u8],
    pub rows: usize,
    pub row_size: usize,
    /// Start of the variable section (the marker itself).
    var: usize,
}

fn le<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N]> {
    Ok(b.get(off..off + N).context("read past end of table")?.try_into()?)
}

impl<'a> Table<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let rows = u32::from_le_bytes(le(data, 0)?) as usize;
        let var = data
            .windows(8)
            .position(|w| w == MARKER)
            .context("no 0xBB marker; not a datc64 table")?;
        ensure!(var >= 4, "marker before row data");
        let fixed = var - 4;
        let row_size = if rows == 0 { 0 } else { fixed / rows };
        ensure!(rows == 0 || fixed % rows == 0, "fixed section not a whole number of rows");
        Ok(Self { data, rows, row_size, var })
    }

    pub fn row(&self, i: usize) -> Result<&'a [u8]> {
        ensure!(i < self.rows, "row {i} out of range ({} rows)", self.rows);
        let s = 4 + i * self.row_size;
        Ok(&self.data[s..s + self.row_size])
    }

    fn string_at(&self, off: u64) -> Result<String> {
        let start = self.var + off as usize;
        let tail = self.data.get(start..).context("string offset out of range")?;
        let units: Vec<u16> = tail
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    fn scalar(&self, ty: &Ty, b: &[u8], off: usize) -> Result<Value> {
        Ok(match ty {
            Ty::Bool => Value::Bool(*b.get(off).context("short row")? != 0),
            Ty::I16 => Value::Int(i16::from_le_bytes(le(b, off)?) as i64),
            Ty::U16 => Value::Int(u16::from_le_bytes(le(b, off)?) as i64),
            Ty::I32 => Value::Int(i32::from_le_bytes(le(b, off)?) as i64),
            Ty::U32 | Ty::Enum(_) => Value::Int(u32::from_le_bytes(le(b, off)?) as i64),
            Ty::F32 => Value::Float(f32::from_le_bytes(le(b, off)?)),
            Ty::String => Value::Str(self.string_at(u64::from_le_bytes(le(b, off)?))?),
            Ty::Rid => Value::Int(u64::from_le_bytes(le(b, off)?) as i64),
            Ty::Ref(_) => {
                let k = u64::from_le_bytes(le(b, off)?);
                Value::Ref((k != NULL_ROW).then_some(k))
            }
            Ty::Raw(_) => bail!("raw column has no value"),
            Ty::List(_) => bail!("nested list"),
        })
    }

    /// Decode one cell. `b` is the row, `off` the column's offset in it.
    pub fn cell(&self, ty: &Ty, b: &[u8], off: usize) -> Result<Value> {
        match ty {
            Ty::List(inner) => {
                let count = u64::from_le_bytes(le(b, off)?) as usize;
                let start = u64::from_le_bytes(le(b, off + 8)?) as usize;
                let esz = inner.size();
                ensure!(count < 1 << 20, "implausible list length {count}");
                let base = self.var + start;
                let region = self
                    .data
                    .get(base..base + count * esz)
                    .context("list data out of range")?;
                let items = (0..count)
                    .map(|i| self.scalar(inner, region, i * esz))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::List(items))
            }
            other => self.scalar(other, b, off),
        }
    }

    /// Decode a full row using `schema`; anonymous columns are skipped.
    pub fn decode_row(&self, schema: &TableSchema, i: usize) -> Result<Vec<(String, Value)>> {
        let row = self.row(i)?;
        schema
            .columns
            .iter()
            .filter_map(|c| c.name.as_ref().map(|n| (n, c)))
            .map(|(n, c)| Ok((n.clone(), self.cell(&c.ty, row, c.offset)?)))
            .collect()
    }
}
