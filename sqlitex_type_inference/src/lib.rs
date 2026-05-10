use std::collections::{HashMap, HashSet};

use crate::expr::{BaseType, sqlite_datatype_to_base_type};
use crate::table::{ColumnInfo, normalize_identifier};
use sqlparser::ast::{ColumnOption, ColumnOptionDef, DataType, Expr, Value, Visit, Visitor};
use sqlparser::{ast::Statement, dialect::SQLiteDialect, parser::Parser};
use std::ops::ControlFlow;
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
            // check is done for external .sql file
            let is_strict = create.strict;
            for col in &create.columns {
                let is_bool = matches!(&col.data_type, DataType::Boolean | DataType::Bool);
                if is_strict && is_bool {
                    return Err(format!(
                        "STRICT tables do not support BOOL/BOOLEAN columns directly. Please use `INTEGER CHECK ({} IN (0, 1))` instead to get the same compile time benefits.",
                        col.name
                    ));
                }

                let base_type = sqlite_datatype_to_base_type(&col.data_type).map_err(|_| {
                    format!(
                        "Unknown type `{}` for column `{}`.",
                        col.data_type, col.name
                    )
                })?;

                // Translate valid sqlite type aliases into their STRICT-compliant equivalents for helpful compiler errors.
                if is_strict {
                    let type_str = col.data_type.to_string().to_uppercase();
                    let allowed_strict = ["INT", "INTEGER", "REAL", "TEXT", "BLOB", "ANY"];

                    if !allowed_strict.contains(&type_str.as_str()) {
                        let suggestion = match base_type {
                            BaseType::Integer => "INTEGER",
                            BaseType::Real => "REAL",
                            BaseType::Text => "TEXT",
                            BaseType::Blob => "BLOB",
                            _ => unreachable!(),
                        };
                        return Err(format!(
                            "'{}' has '{}' type which is not supported in STRICT Tables. Use '{}' instead.",
                            col.name, type_str, suggestion
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn is_create_table(sql: &str) -> bool {
    Parser::parse_sql(&SQLiteDialect {}, sql)
        .ok()
        .and_then(|ast| ast.into_iter().next())
        .map(|stmt| matches!(stmt, Statement::CreateTable(_)))
        .unwrap_or(false)
}

pub fn validate_no_virtual_tables(sql: &str) -> Result<(), String> {
    let Ok(statements) = Parser::parse_sql(&SQLiteDialect {}, sql) else {
        return Ok(());
    };

    for statement in statements {
        if let Statement::CreateVirtualTable { .. } = statement {
            return Err("Creation of Virtual tables are not support at compile time. Resort to runtime features instead.".to_string());
        }
    }

    Ok(())
}
pub fn validate_single_statement(sql: &str) -> Result<(), String> {
    let dialect = SQLiteDialect {};
    if let Ok(ast) = Parser::parse_sql(&dialect, sql)
        && ast.len() > 1
    {
        return Err(
            "Multiple SQL statements detected. Split them into separate sql!() macros.".to_string(),
        );
    }
    Ok(())
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

pub fn rewrite_bool_columns(sql: &str) -> Result<String, String> {
    let dialect = SQLiteDialect {};

    let Ok(mut ast) = Parser::parse_sql(&dialect, sql) else {
        return Ok(sql.to_string());
    };

    for stmt in &mut ast {
        if let Statement::CreateTable(create) = stmt {
            // check is done for sql!() macro
            let is_strict = create.strict;
            for col in &mut create.columns {
                let is_bool = matches!(&col.data_type, DataType::Boolean | DataType::Bool);
                if is_bool {
                    if is_strict {
                        return Err(format!(
                            "STRICT tables do not support BOOL/BOOLEAN columns directly. Please use `INTEGER CHECK ({} IN (0, 1))` instead to get the same compile time benefits.",
                            col.name
                        ));
                    }
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

    Ok(ast
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join(";\n"))
}

pub fn validate_sql_file_syntax(sql: &str) -> Result<(), String> {
    Parser::parse_sql(&SQLiteDialect {}, sql)
        .map_err(|e| format!("Invalid SQL syntax in schema file: {}", e))?;
    Ok(())
}

pub fn run_qusql_fallback(
    sql: &str,
    all_tables: &std::collections::HashMap<String, Vec<crate::table::ColumnInfo>>,
) -> Result<(Vec<crate::table::ColumnInfo>, Vec<crate::expr::Type>), String> {
    use qusql_type::{schema::parse_schemas, type_statement, TypeOptions, SQLDialect, SQLArguments, StatementType, Issues};
    let mut ddl = String::new();
    for (table_name, cols) in all_tables {
        ddl.push_str(&format!("CREATE TABLE {} (\n", table_name));
        let col_defs: Vec<String> = cols.iter().map(|c| {
            let t = match c.data_type.base_type {
                crate::expr::BaseType::Integer => "INTEGER",
                crate::expr::BaseType::Real => "REAL",
                crate::expr::BaseType::Text => "TEXT",
                crate::expr::BaseType::Blob => "BLOB",
                crate::expr::BaseType::Bool => "BOOLEAN",
                _ => "TEXT",
            };
            let nn = if c.data_type.nullable { "" } else { "NOT NULL" };
            format!("{} {} {}", c.name, t, nn)
        }).collect();
        ddl.push_str(&col_defs.join(",\n"));
        ddl.push_str("\n);\n");
    }

    let opts = TypeOptions::new()
        .dialect(SQLDialect::Sqlite)
        .arguments(SQLArguments::QuestionMark);
    let mut schema_issues = Issues::new(&ddl);
    let schemas = parse_schemas(&ddl, &mut schema_issues, &opts);

    let mut query_issues = Issues::new(sql);
    let stmt_type = type_statement(&schemas, sql, &mut query_issues, &opts);

    if !query_issues.is_ok() {
        return Err(format!("Qusql failed:\n{}", query_issues));
    }

    let map_type = |ft: &qusql_type::FullType| -> crate::expr::Type {
        let type_str = ft.to_string().to_lowercase();
        let base = if type_str.contains("int") || type_str.contains("i8") || type_str.contains("i16") || type_str.contains("i32") || type_str.contains("i64") || type_str.contains("u8") || type_str.contains("u16") || type_str.contains("u32") || type_str.contains("u64") {
            crate::expr::BaseType::Integer
        } else if type_str.contains("float") || type_str.contains("double") || type_str.contains("real") || type_str.contains("f32") || type_str.contains("f64") {
            crate::expr::BaseType::Real
        } else if type_str.contains("bool") {
            crate::expr::BaseType::Bool
        } else if type_str.contains("blob") || type_str.contains("byte") || type_str.contains("binary") || type_str.contains("geometry") {
            crate::expr::BaseType::Blob
        } else if type_str.contains("any") {
            crate::expr::BaseType::Unknowns
        } else {
            crate::expr::BaseType::Text
        };
        crate::expr::Type {
            base_type: base,
            nullable: !ft.not_null,
            contains_placeholder: false,
        }
    };

    match stmt_type {
        StatementType::Select { columns, arguments } => {
            let cols = columns.into_iter().enumerate().map(|(i, c)| {
                let mut mapped = map_type(&c.type_);
                let name = c.name.as_ref().map(|id| id.to_string()).unwrap_or_else(|| format!("col_{}", i));

                if mapped.base_type == crate::expr::BaseType::Integer {
                    for tcols in all_tables.values() {
                        if let Some(custom) = tcols.iter().find(|tc| tc.name == name)
                            && custom.data_type.base_type == crate::expr::BaseType::Bool {
                                mapped.base_type = crate::expr::BaseType::Bool;
                                break;
                            }
                    }
                }
                crate::table::ColumnInfo {
                    name,
                    data_type: mapped,
                    has_default: false,
                }
            }).collect();
            let args = arguments.iter().map(|(_, ft)| map_type(ft)).collect();
            Ok((cols, args))
        },
        StatementType::Insert { arguments, .. } |
        StatementType::Replace { arguments, .. } |
        StatementType::Update { arguments, .. } |
        StatementType::Delete { arguments, .. } |
        StatementType::Call { arguments, .. } => {
            Ok((vec![], arguments.iter().map(|(_, ft)| map_type(ft)).collect()))
        },
        _ => Ok((vec![], vec![]))
    }
}



