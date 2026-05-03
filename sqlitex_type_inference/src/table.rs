use std::collections::HashMap;
use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, ColumnOption, CreateTable, Expr, ObjectNamePart, Statement, visit_relations,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use crate::expr::{BaseType, Type, sqlite_datatype_to_base_type};

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: Type,
    pub has_default: bool,
}

pub fn normalize_identifier(ident: &sqlparser::ast::Ident) -> String {
    match ident.quote_style {
        Some(_) => ident.value.clone(),     // Keep "MyTable" as "MyTable"
        None => ident.value.to_lowercase(), // Convert MyTable -> mytable
    }
}

pub fn normalize_part(part: &ObjectNamePart) -> String {
    match part {
        ObjectNamePart::Identifier(ident) => normalize_identifier(ident),
        _ => part.to_string(), // Fallback for wildcards etc.
    }
}

/// Bool type derived from CHECK constraint
/// 1. CHECK (col IN (0, 1))
/// 2. CHECK (col = 0 OR col = 1)
fn is_boolean_constraint(expr: &Expr) -> bool {
    match expr {
        // CHECK (col IN (0, 1))
        Expr::InList { list, .. } => {
            if list.len() != 2 {
                return false;
            }
            let has_zero = list.iter().any(|e| e.to_string() == "0");
            let has_one = list.iter().any(|e| e.to_string() == "1");
            has_zero && has_one
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let is_eq_check = |op_expr: &Expr, target_val: &str| -> bool {
                let inner = if let Expr::Nested(n) = op_expr {
                    n
                } else {
                    op_expr
                };
                match inner {
                    Expr::BinaryOp {
                        left,
                        op: BinaryOperator::Eq,
                        right,
                    } => left.to_string() == target_val || right.to_string() == target_val,
                    _ => false,
                }
            };

            let has_zero = is_eq_check(left, "0") || is_eq_check(right, "0");
            let has_one = is_eq_check(left, "1") || is_eq_check(right, "1");

            has_zero && has_one
        }
        _ => false,
    }
}

pub fn create_tables(sql: &str, tables: &mut HashMap<String, Vec<ColumnInfo>>) {
    let dialect = SQLiteDialect {};
    let Ok(ast) = Parser::parse_sql(&dialect, sql) else {
        return;
    };

    for statement in ast {
        if let Statement::CreateTable(CreateTable {
            name,
            columns,
            without_rowid,
            ..
        }) = statement
        {
            let table_name = name
                .0
                .last()
                .map(normalize_part)
                .unwrap_or(name.to_string());

            let table_columns = columns
                .iter()
                .map(|col| {
                    let mut nullable = true;
                    let mut is_detected_boolean = false;
                    let mut is_default = false;

                    // check if type is strictly INTEGER (not INT)
                    let is_strictly_integer =
                        col.data_type.to_string().eq_ignore_ascii_case("INTEGER");

                    for option_def in &col.options {
                        match &option_def.option {
                            ColumnOption::Check(expr)

                                if is_boolean_constraint(expr) => {
                                    is_detected_boolean = true;
                                }
                            ColumnOption::NotNull => {
                                nullable = false;
                            }
                            ColumnOption::Unique {
                                is_primary: true, ..
                            }
                                // "INTEGER PRIMARY KEY" is an alias for ROWID (auto-increment)
                                // UNLESS the table is declared WITHOUT ROWID.
                                if is_strictly_integer && !without_rowid => {
                                    is_default = true;
                                }
                            ColumnOption::Default(_) => is_default = true,

                            // Check for explicit AUTOINCREMENT token
                            ColumnOption::DialectSpecific(tokens)
                                if tokens
                                    .iter()
                                    .any(|t| t.to_string().to_uppercase() == "AUTOINCREMENT")
                                => {
                                    is_default = true;
                                }
                            _ => {}
                        }
                    }

                    let base_type = if is_detected_boolean {
                        BaseType::Bool
                    } else {
                        sqlite_datatype_to_base_type(&col.data_type)
                            .unwrap_or(BaseType::Null)
                    };

                    ColumnInfo {
                        name: normalize_identifier(&col.name),
                        data_type: Type {
                            base_type,
                            nullable,
                            contains_placeholder: false,
                        },
                        has_default: is_default,
                    }
                })
                .collect();

            tables.insert(table_name.to_lowercase(), table_columns);
        }
    }
}

#[allow(unused)]
pub fn get_table_names(sql: &str) -> Vec<String> {
    let Ok(statements) = Parser::parse_sql(&SQLiteDialect {}, sql) else {
        return vec![];
    };

    let mut visited = vec![];
    let _ = visit_relations(&statements, |expr| {
        let name = expr
            .0
            .last()
            .map(normalize_part)
            .unwrap_or(expr.to_string());
        visited.push(name);
        ControlFlow::<()>::Continue(())
    });
    visited
}
