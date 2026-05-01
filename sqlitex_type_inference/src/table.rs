use std::collections::HashMap;
use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, ColumnOption, CreateTable, Expr, ObjectNamePart, Statement, visit_relations,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use crate::expr::{BaseType, Type};

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: Type,
    pub check_constraint: Option<String>,
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

fn convert_sqlite_to_rust_type(sql: String, nullable: bool, is_bool_context: bool) -> Type {
    let sql_upper = sql.to_uppercase();

    if is_bool_context || sql_upper.contains("BOOL") {
        Type {
            base_type: BaseType::Bool,
            nullable,
            contains_placeholder: false,
        }
    } else if sql_upper.contains("INT") {
        Type {
            base_type: BaseType::Integer,
            nullable,
            contains_placeholder: false,
        }
    } else if sql_upper.contains("REAL")
        || sql_upper.contains("FLOAT")
        || sql_upper.contains("DOUBLE")
    {
        Type {
            base_type: BaseType::Real,
            nullable,
            contains_placeholder: false,
        }
    } else if sql_upper.contains("TEXT") {
        Type {
            base_type: BaseType::Text,
            nullable,
            contains_placeholder: false,
        }
    } else if sql_upper.contains("BLOB") {
        Type {
            base_type: BaseType::Blob,
            nullable,
            contains_placeholder: false,
        }
    } else {
        Type {
            base_type: BaseType::Null,
            nullable,
            contains_placeholder: false,
        }
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
                    let mut check_expr_str = None;
                    let mut nullable = true;
                    let mut is_detected_boolean = false;
                    let mut is_default = false;

                    // check if type is strictly INTEGER (not INT)
                    let is_strictly_integer =
                        col.data_type.to_string().eq_ignore_ascii_case("INTEGER");

                    for option_def in &col.options {
                        match &option_def.option {
                            ColumnOption::Check(expr) => {
                                check_expr_str = Some(expr.to_string());

                                if is_boolean_constraint(expr) {
                                    is_detected_boolean = true;
                                }
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

                    let data_type = convert_sqlite_to_rust_type(
                        col.data_type.to_string(),
                        nullable,
                        is_detected_boolean,
                    );

                    ColumnInfo {
                        name: normalize_identifier(&col.name),
                        data_type,
                        check_constraint: check_expr_str,
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