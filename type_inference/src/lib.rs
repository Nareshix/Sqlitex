use std::collections::{HashMap, HashSet};

use sqlparser::ast::{
    ColumnOption, ColumnOptionDef, DataType, Expr, Value, Visit, Visitor,
};
use sqlparser::{ast::Statement, dialect::GenericDialect, dialect::SQLiteDialect, parser::Parser};
use std::ops::ControlFlow;
use crate::expr::sqlite_datatype_to_base_type;
use crate::table::{ColumnInfo, normalize_identifier, normalize_part};
pub mod binding_patterns;
pub mod expr;
pub mod select_patterns;
pub mod table;

struct CastChecker {
    error: Option<String>,
}

impl Visitor for CastChecker {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Cast { data_type, .. } = expr
            && let DataType::Custom(name, _) = data_type
        {
            self.error = Some(format!("Unknown type casting: `{}`.", name));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

pub fn validate_cast_types(sql: &str) -> Result<(), String> {
    let dialect = SQLiteDialect {};
    let Ok(statements) = Parser::parse_sql(&dialect, sql) else {
        return Ok(());
    };

    let mut checker = CastChecker { error: None };
    let _ = statements.visit(&mut checker);

    if let Some(err) = checker.error {
        return Err(err);
    }

    Ok(())
}

pub fn validate_create_table_types(sql: &str) -> Result<(), String> {
    let Ok(statements) = Parser::parse_sql(&SQLiteDialect {}, sql) else {
        return Ok(());
    };

    for statement in &statements {
        if let Statement::CreateTable(create) = statement {
            for col in &create.columns {
                sqlite_datatype_to_base_type(&col.data_type)
                    .map_err(|_| format!(
                        "Unknown type `{}` for column `{}`.",
                        col.data_type, col.name
                    ))?;
            }
        }
    }
    Ok(())
}




pub fn validate_single_statement(sql: &str) -> bool {
    let dialect = SQLiteDialect {};
    if let Ok(ast) = Parser::parse_sql(&dialect, sql)
        && ast.len() > 1
    {
        return false;
    }
    true
}

pub fn validate_insert_strict(
    sql: &str,
    tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<(), String> {
    let dialect = SQLiteDialect {};
    let ast = Parser::parse_sql(&dialect, sql).map_err(|e| e.to_string())?;

    for statement in ast {
        if let Statement::Insert(insert) = statement {
            let raw_table_name = insert.table.to_string();

            // Normalize table name (handle "public.users" -> "users")
            let t_name = raw_table_name
                .split('.')
                .next_back()
                .unwrap_or(&raw_table_name)
                .to_lowercase();

            let schema_cols = match tables.get(&t_name) {
                Some(cols) => cols,
                None => return Err(format!("Table '{}' does not exist", t_name)),
            };

            // Implicit Insert (No columns specified) are allowed
            if insert.columns.is_empty() {
                continue;
            }

            let provided_names = insert.columns.iter().map(normalize_identifier).collect();

            let mandatory_names: HashSet<_> = schema_cols
                .iter()
                .filter(|col| !col.has_default)
                .map(|col| col.name.clone())
                .collect();

            let missing: Vec<_> = mandatory_names.difference(&provided_names).collect();

            if !missing.is_empty() {
                return Err(format!(
                    "Missing mandatory columns (columns with no default/autoincrement): {:?}",
                    missing
                ));
            }
        }
    }

    Ok(())
}

pub fn pg_cast_syntax_to_sqlite(sql: &str) -> String {
    let mut chars: Vec<char> = sql.chars().collect();
    let mut i = 0;

    let mut cast_indices = Vec::new();

    let mut in_quote = false;
    let mut quote_char = '\0';
    let mut in_comment = false;

    while i < chars.len() {
        let c = chars[i];
        let next_c = if i + 1 < chars.len() {
            chars[i + 1]
        } else {
            '\0'
        };

        if in_comment {
            if c == '\n' {
                in_comment = false;
            }
        } else if in_quote {
            if c == quote_char {
                if next_c == quote_char {
                    i += 1;
                } else {
                    in_quote = false;
                }
            }
        } else if c == '-' && next_c == '-' {
            in_comment = true;
            i += 1;
        } else if c == '\'' || c == '"' {
            in_quote = true;
            quote_char = c;
        } else if c == ':' && next_c == ':' {
            cast_indices.push(i);
            i += 1;
        }
        i += 1;
    }

    for &idx in cast_indices.iter().rev() {
        let mut rhs_end = idx + 2;

        while rhs_end < chars.len() && chars[rhs_end].is_whitespace() {
            rhs_end += 1;
        }

        let mut p_depth = 0;
        while rhs_end < chars.len() {
            let c = chars[rhs_end];

            if p_depth == 0 {
                if c.is_whitespace() {
                    break;
                }
                if ",);".contains(c) {
                    break;
                }
                if "+-*/=<>!^%|~".contains(c) {
                    break;
                }
            }

            if c == '(' {
                p_depth += 1;
            }
            if c == ')' {
                p_depth -= 1;
            }
            rhs_end += 1;
        }

        let mut lhs_start = idx;

        // Skip initial spaces
        while lhs_start > 0 && chars[lhs_start - 1].is_whitespace() {
            lhs_start -= 1;
        }

        if lhs_start > 0 {
            let end_char = chars[lhs_start - 1];

            if end_char == ')' {
                // Balance parenthesis backwards
                let mut balance = 1;
                lhs_start -= 1;
                while lhs_start > 0 && balance > 0 {
                    lhs_start -= 1;
                    if chars[lhs_start] == ')' {
                        balance += 1;
                    }
                    if chars[lhs_start] == '(' {
                        balance -= 1;
                    }
                }
            } else if end_char == '\'' || end_char == '"' {
                // Handle quoted strings/identifiers backwards
                let q = end_char;
                lhs_start -= 1;
                while lhs_start > 0 {
                    lhs_start -= 1;
                    if chars[lhs_start] == q {
                        // Check for escaped quote (e.g. 'Don''t')
                        if lhs_start > 0 && chars[lhs_start - 1] == q {
                            lhs_start -= 1;
                        } else {
                            break;
                        }
                    }
                }
            } else {
                while lhs_start > 0 {
                    let c = chars[lhs_start - 1];

                    if c.is_whitespace() {
                        break;
                    }
                    if ",();".contains(c) {
                        break;
                    }
                    if "+-*/=<>!^%|~".contains(c) {
                        break;
                    }

                    lhs_start -= 1;
                }
            }
        }

        let val: String = chars[lhs_start..idx].iter().collect();
        let type_name: String = chars[(idx + 2)..rhs_end].iter().collect();
        let new_str = format!("CAST({} AS {})", val.trim(), type_name.trim());

        chars.splice(lhs_start..rhs_end, new_str.chars());
    }

    chars.into_iter().collect()
}

pub fn rewrite_bool_columns(sql: &str) -> String {
    let dialect = GenericDialect {};

    let Ok(mut ast) = Parser::parse_sql(&dialect, sql) else {
        return sql.to_string();
    };

    for stmt in &mut ast {
        if let Statement::CreateTable(create) = stmt {
            for col in &mut create.columns {

                let is_bool = matches!(&col.data_type, DataType::Boolean | DataType::Bool);
                if is_bool {
                    col.data_type = DataType::Integer(None);

                    let check_expr = Expr::InList {
                        expr: Box::new(Expr::Identifier(col.name.clone())),
                        list: vec![
                            Expr::Value(Value::Number("0".to_string(), false).into()),
                            Expr::Value(Value::Number("1".to_string(), false).into()),
                        ],
                        negated: false,
                    };

                    col.options.push(ColumnOptionDef {
                        name: None,
                        option: ColumnOption::Check(check_expr),
                    });
                }
            }
        }
    }

    ast.iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join(";\n")
}
