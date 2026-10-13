//! The narrow SQL surface the run engine is written against.
//!
//! The engine in [`crate::engine`] is one body of code for every backend. It
//! speaks a small subset of what `rusqlite` offers, with the same shape, so the
//! SQLite backend is a thin adapter and the Postgres one is another. Every
//! statement the engine writes is text with `?N` placeholders, and every value
//! is one of four kinds, which both databases hold without a conversion that
//! could lose anything.
//!
//! It is synchronous on purpose. The engine's functions call each other
//! recursively (a finishing child moves its parent, which can finish it and
//! cancel its other children), and that recursion, written as `async`, is a
//! tree of boxed futures that is harder to read and no faster: every backend
//! here runs one transaction on one connection at a time anyway.

use crate::Error;
use std::fmt;

/// Which SQL a backend speaks, for the few expressions the two write
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Sqlite,
    #[cfg_attr(not(feature = "postgres"), allow(dead_code))]
    Postgres,
}

/// One bound value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Arg<'a> {
    Null,
    Int(i64),
    Real(f64),
    Text(&'a str),
}

/// Anything that binds as one [`Arg`].
pub(crate) trait ToArg {
    fn to_arg(&self) -> Arg<'_>;
}

impl ToArg for str {
    fn to_arg(&self) -> Arg<'_> {
        Arg::Text(self)
    }
}

impl ToArg for String {
    fn to_arg(&self) -> Arg<'_> {
        Arg::Text(self)
    }
}

impl ToArg for i64 {
    fn to_arg(&self) -> Arg<'_> {
        Arg::Int(*self)
    }
}

impl ToArg for u32 {
    fn to_arg(&self) -> Arg<'_> {
        Arg::Int(i64::from(*self))
    }
}

impl ToArg for f64 {
    fn to_arg(&self) -> Arg<'_> {
        Arg::Real(*self)
    }
}

impl<T: ToArg + ?Sized> ToArg for &T {
    fn to_arg(&self) -> Arg<'_> {
        (**self).to_arg()
    }
}

impl<T: ToArg> ToArg for Option<T> {
    fn to_arg(&self) -> Arg<'_> {
        self.as_ref().map_or(Arg::Null, ToArg::to_arg)
    }
}

/// Bind a list of values, in the shape `rusqlite::params!` has.
macro_rules! params {
    () => {
        &[] as &[$crate::db::Arg<'_>]
    };
    ($($x:expr),+ $(,)?) => {
        &[$($crate::db::ToArg::to_arg(&$x)),+] as &[$crate::db::Arg<'_>]
    };
}
pub(crate) use params;

/// Bind every item of an iterator, in order.
pub(crate) fn params_from_iter<'a, T: ToArg + 'a>(
    items: impl IntoIterator<Item = &'a T>,
) -> Vec<Arg<'a>> {
    items.into_iter().map(ToArg::to_arg).collect()
}

/// One value read back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Val {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

/// One row read back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row(pub(crate) Vec<Val>);

/// Something that names a column of a [`Row`]. Only a position does.
pub(crate) trait RowIndex {
    fn index(self) -> usize;
}

impl RowIndex for usize {
    fn index(self) -> usize {
        self
    }
}

/// A Rust value one column reads as.
pub(crate) trait FromVal: Sized {
    fn from_val(v: &Val) -> Option<Self>;
}

impl FromVal for i64 {
    fn from_val(v: &Val) -> Option<Self> {
        match v {
            Val::Int(i) => Some(*i),
            Val::Null | Val::Real(_) | Val::Text(_) => None,
        }
    }
}

impl FromVal for f64 {
    fn from_val(v: &Val) -> Option<Self> {
        match v {
            Val::Real(f) => Some(*f),
            // An integral column value, which a REAL column in SQLite can hand
            // back after an exact sum.
            #[allow(clippy::cast_precision_loss)]
            Val::Int(i) => Some(*i as f64),
            Val::Null | Val::Text(_) => None,
        }
    }
}

impl FromVal for String {
    fn from_val(v: &Val) -> Option<Self> {
        match v {
            Val::Text(s) => Some(s.clone()),
            Val::Null | Val::Int(_) | Val::Real(_) => None,
        }
    }
}

impl<T: FromVal> FromVal for Option<T> {
    fn from_val(v: &Val) -> Option<Self> {
        match v {
            Val::Null => Some(None),
            Val::Int(_) | Val::Real(_) | Val::Text(_) => T::from_val(v).map(Some),
        }
    }
}

impl Row {
    /// One column, in the shape `rusqlite::Row::get` has.
    pub(crate) fn get<I: RowIndex, T: FromVal>(&self, idx: I) -> DbResult<T> {
        let i = idx.index();
        let v = self.0.get(i).ok_or_else(|| {
            DbError::Conversion(format!("column {i} is past the {} the row holds", self.0.len()))
        })?;
        T::from_val(v).ok_or_else(|| {
            DbError::Conversion(format!(
                "column {i} holds {v:?}, which does not read as {}",
                std::any::type_name::<T>()
            ))
        })
    }
}

/// What a statement can fail with.
#[derive(Debug)]
pub(crate) enum DbError {
    /// `query_row` found nothing. `optional()` turns this into `None`.
    NoRows,
    /// A column did not read as the type asked for.
    Conversion(String),
    /// The backend refused or failed. An [`Error::Retry`] here is a
    /// transaction that lost to a concurrent one and is safe to run again.
    Backend(Error),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRows => f.write_str("the query returned no rows"),
            Self::Conversion(m) => f.write_str(m),
            Self::Backend(e) => e.fmt(f),
        }
    }
}

impl From<DbError> for Error {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Backend(e) => e,
            DbError::NoRows | DbError::Conversion(_) => Self::Backend(e.to_string()),
        }
    }
}

pub(crate) type DbResult<T> = std::result::Result<T, DbError>;

/// `rusqlite::OptionalExtension`, for this surface.
pub(crate) trait OptionalExtension<T> {
    fn optional(self) -> DbResult<Option<T>>;
}

impl<T> OptionalExtension<T> for DbResult<T> {
    fn optional(self) -> DbResult<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(DbError::NoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// A connection or a transaction: something statements run against.
pub(crate) trait Db {
    /// Run a statement and answer how many rows it changed.
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize>;
    /// Run a query and answer every row.
    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>>;
    /// Which SQL this backend speaks.
    fn dialect(&self) -> Dialect;
}

/// The calls the engine makes that take a closure, so they cannot sit on the
/// object-safe [`Db`] itself.
pub(crate) trait DbExt: Db {
    /// The first row, mapped, or [`DbError::NoRows`].
    fn query_row<T, F>(&self, sql: &str, params: &[Arg<'_>], f: F) -> DbResult<T>
    where
        F: FnOnce(&Row) -> DbResult<T>,
    {
        match self.query(sql, params)?.first() {
            Some(row) => f(row),
            None => Err(DbError::NoRows),
        }
    }

    /// A statement to run more than once, in the shape `rusqlite` has.
    fn prepare(&self, sql: &str) -> DbResult<Statement<'_, Self>> {
        Ok(Statement { db: self, sql: sql.to_owned() })
    }
}

impl<D: Db + ?Sized> DbExt for D {}

/// A statement bound to the connection it runs on.
pub(crate) struct Statement<'a, D: ?Sized> {
    db: &'a D,
    sql: String,
}

impl<D: Db + ?Sized> Statement<'_, D> {
    /// Every row, mapped.
    pub(crate) fn query_map<T, F>(
        &mut self,
        params: &[Arg<'_>],
        f: F,
    ) -> DbResult<std::vec::IntoIter<DbResult<T>>>
    where
        F: FnMut(&Row) -> DbResult<T>,
    {
        let rows = self.db.query(&self.sql, params)?;
        Ok(rows.iter().map(f).collect::<Vec<_>>().into_iter())
    }
}

/// A connection that can open a transaction.
pub(crate) trait Conn: Db {
    /// Open one. Dropping it without [`Tx::commit`] rolls it back.
    fn transaction(&mut self) -> DbResult<Box<dyn Tx + '_>>;

    /// How many times one transaction that lost to a concurrent one is run
    /// again in place before the loss is handed up. One for a backend with a
    /// single writer, which never loses that way.
    fn tx_attempts(&self) -> usize;

    /// Wait before running a lost transaction again, so two that keep
    /// colliding stop doing it in step.
    fn backoff(&self, attempt: usize);
}

/// An open transaction.
pub(crate) trait Tx: Db {
    fn commit(self: Box<Self>) -> DbResult<()>;
}

impl Db for Box<dyn Tx + '_> {
    fn execute(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<usize> {
        (**self).execute(sql, params)
    }

    fn query(&self, sql: &str, params: &[Arg<'_>]) -> DbResult<Vec<Row>> {
        (**self).query(sql, params)
    }

    fn dialect(&self) -> Dialect {
        (**self).dialect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_bind_every_kind() {
        let name = String::from("a");
        let none: Option<i64> = None;
        let p = params![name, 7i64, 1.5f64, none, Some("b"), 3u32];
        assert_eq!(
            p,
            &[Arg::Text("a"), Arg::Int(7), Arg::Real(1.5), Arg::Null, Arg::Text("b"), Arg::Int(3)]
        );
    }

    #[test]
    fn a_column_of_the_wrong_kind_is_an_error_not_a_default() {
        let row = Row(vec![Val::Text("x".into()), Val::Null, Val::Int(2)]);
        assert!(row.get::<_, i64>(0).is_err());
        assert_eq!(row.get::<_, Option<String>>(1).unwrap(), None);
        assert_eq!(row.get::<_, f64>(2).unwrap(), 2.0);
        assert!(row.get::<_, String>(9).is_err());
    }
}
