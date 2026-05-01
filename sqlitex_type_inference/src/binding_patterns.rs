use crate::expr::{BaseType, Type, evaluate_expr_type};
use crate::pg_cast_syntax_to_sqlite;
use crate::table::{ColumnInfo, get_table_names, normalize_identifier, normalize_part};
use sqlparser::ast::{
    BinaryOperator, DataType, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, SetExpr,
    Spanned, Statement,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: u64,
    pub column: u64,
}

impl From<sqlparser::tokenizer::Location> for Location {
    fn from(loc: sqlparser::tokenizer::Location) -> Self {
        Self {
            line: loc.line,
            column: loc.column,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InferenceError {
    pub start: Location,
    pub end: Location,
    pub message: String,
}

fn get_table_name_str(table: &sqlparser::ast::TableObject) -> String {
    match table {
        sqlparser::ast::TableObject::TableName(obj_name) => {
            obj_name.0.last().map(normalize_part).unwrap_or_default()
        }
        _ => String::new(),
    }
}

fn err_from_expr(expr: &impl Spanned, msg: impl Into<String>) -> InferenceError {
    let span = expr.span();
    InferenceError {
        start: span.start.into(),
        end: span.end.into(),
        message: msg.into(),
    }
}

#[allow(unused)]
pub fn get_type_of_binding_parameters(
    sql: &str,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<Vec<Type>, InferenceError> {
    let ast = Parser::parse_sql(&SQLiteDialect {}, sql).map_err(|e| InferenceError {
        start: Location { line: 1, column: 1 },
        end: Location { line: 1, column: 1 },
        message: format!("SQL syntax error: {}", e),
    })?;

    if ast.is_empty() {
        return Ok(vec![]);
    }

    let statement = &ast[0];
    let sql = pg_cast_syntax_to_sqlite(sql);

    let table_names = get_table_names(&sql);
    let mut results = Vec::new();

    let bool_hint = Some(Type {
        base_type: BaseType::Bool,
        nullable: false,
        contains_placeholder: false,
    });

    match statement {
        Statement::Query(q) => {
            traverse_query(q, &table_names, all_tables, &mut results)?;
        }

        Statement::Delete(delete_node) => {
            if let Some(expr) = &delete_node.selection {
                traverse_expr(
                    expr,
                    &table_names,
                    all_tables,
                    &mut results,
                    bool_hint.clone(),
                )?;

                traverse_returning(
                    &delete_node.returning,
                    &table_names,
                    all_tables,
                    &mut results,
                )?;
            }
        }

        Statement::Update {
            assignments,
            selection,
            returning,
            table,
            limit,
            from,
            or,
        } => {
            let table_name = match &table.relation {
                sqlparser::ast::TableFactor::Table { name, .. } => {
                    name.0.last().map(normalize_part).unwrap_or_default()
                }
                _ => table.relation.to_string(),
            };

            for assignment in assignments {
                match &assignment.target {
                    sqlparser::ast::AssignmentTarget::ColumnName(obj_name) => {
                        let col_name = obj_name.0.last().map(normalize_part).unwrap_or_default();

                        let mut hint = None;
                        if let Some(cols) = all_tables.get(&table_name)
                            && let Some(col_info) = cols.iter().find(|c| c.name == col_name)
                        {
                            hint = Some(col_info.data_type.clone());
                        }
                        traverse_expr(
                            &assignment.value,
                            &table_names,
                            all_tables,
                            &mut results,
                            hint,
                        )
                        .map_err(|mut e| {
                            e.message = format!(
                                "{} in UPDATE assignment to column '{}'",
                                e.message, col_name
                            );
                            e
                        })?;
                    }
                    sqlparser::ast::AssignmentTarget::Tuple(col_names) => {
                        if let sqlparser::ast::Expr::Tuple(values) = &assignment.value {
                            // Zip pairs the two iterators together instantly
                            for (name_obj, val_expr) in col_names.iter().zip(values.iter()) {
                                let col_name =
                                    name_obj.0.last().map(|p| p.to_string()).unwrap_or_default();

                                let hint = all_tables
                                    .get(&table_name)
                                    .and_then(|cols| cols.iter().find(|c| c.name == col_name))
                                    .map(|c| c.data_type.clone());

                                traverse_expr(
                                    val_expr,
                                    &table_names,
                                    all_tables,
                                    &mut results,
                                    hint,
                                )?;
                            }
                        } else {
                            traverse_expr(
                                &assignment.value,
                                &table_names,
                                all_tables,
                                &mut results,
                                None,
                            )?;
                        }
                    }
                }
            }
            if let Some(expr) = selection {
                traverse_expr(
                    expr,
                    &table_names,
                    all_tables,
                    &mut results,
                    bool_hint.clone(),
                )?;
            }

            let int_hint = Some(Type {
                base_type: BaseType::Integer,
                nullable: false,
                contains_placeholder: false,
            });

            if let Some(limit_expr) = limit {
                traverse_expr(limit_expr, &table_names, all_tables, &mut results, int_hint)
                    .map_err(|mut e| {
                        e.message = format!("{} in UPDATE LIMIT clause", e.message);
                        e
                    })?;
            }

            if let Some(from_kind) = from {
                let from_tables = match from_kind {
                    sqlparser::ast::UpdateTableFromKind::BeforeSet(tables) => tables,
                    sqlparser::ast::UpdateTableFromKind::AfterSet(tables) => tables,
                };

                for from_table in from_tables {
                    match &from_table.relation {
                        sqlparser::ast::TableFactor::TableFunction { expr, .. } => {
                            traverse_expr(expr, &table_names, all_tables, &mut results, None)?;
                        }
                        sqlparser::ast::TableFactor::Derived { subquery, .. } => {
                            traverse_query(subquery, &table_names, all_tables, &mut results)?;
                        }
                        _ => {}
                    }

                    for join in &from_table.joins {
                        match &join.relation {
                            sqlparser::ast::TableFactor::TableFunction { expr, .. } => {
                                traverse_expr(expr, &table_names, all_tables, &mut results, None)?;
                            }
                            sqlparser::ast::TableFactor::Derived { subquery, .. } => {
                                traverse_query(subquery, &table_names, all_tables, &mut results)?;
                            }
                            _ => {}
                        }

                        let constraint = match &join.join_operator {
                            sqlparser::ast::JoinOperator::Inner(c)
                            | sqlparser::ast::JoinOperator::LeftOuter(c)
                            | sqlparser::ast::JoinOperator::Left(c)
                            | sqlparser::ast::JoinOperator::Right(c)
                            | sqlparser::ast::JoinOperator::RightOuter(c)
                            | sqlparser::ast::JoinOperator::Join(c)
                            | sqlparser::ast::JoinOperator::FullOuter(c) => Some(c),
                            _ => None,
                        };

                        if let Some(sqlparser::ast::JoinConstraint::On(expr)) = constraint {
                            traverse_expr(
                                expr,
                                &table_names,
                                all_tables,
                                &mut results,
                                bool_hint.clone(),
                            )
                            .map_err(|mut e| {
                                e.message =
                                    format!("{} in UPDATE ... FROM join ON clause", e.message);
                                e
                            })?;
                        }
                    }
                }
            }

            traverse_returning(returning, &table_names, all_tables, &mut results)?;
        }

        Statement::Insert(insert_node) => {
            let t_name = get_table_name_str(&insert_node.table);

            let expected_types = if let Some(table_cols) = all_tables.get(&t_name) {
                if insert_node.columns.is_empty() {
                    table_cols
                        .iter()
                        .map(|c| Some(c.data_type.clone()))
                        .collect()
                } else {
                    insert_node
                        .columns
                        .iter()
                        .map(|ident| {
                            table_cols
                                .iter()
                                .find(|c| c.name == normalize_identifier(ident))
                                .map(|c| c.data_type.clone())
                        })
                        .collect()
                }
            } else {
                Vec::new()
            };

            if let Some(source_query) = &insert_node.source {
                match &*source_query.body {
                    SetExpr::Values(values) => {
                        for row in &values.rows {
                            for (idx, expr) in row.iter().enumerate() {
                                let hint = expected_types.get(idx).cloned().flatten();
                                traverse_expr(expr, &table_names, all_tables, &mut results, hint)
                                    .map_err(|mut e| {
                                    e.message = format!(
                                        "{} in INSERT value at column index {}",
                                        e.message, idx
                                    );
                                    e
                                })?;
                            }
                        }
                    }
                    SetExpr::Select(select) => {
                        for (idx, item) in select.projection.iter().enumerate() {
                            let hint = expected_types.get(idx).cloned().flatten();
                            match item {
                                sqlparser::ast::SelectItem::UnnamedExpr(expr)
                                | sqlparser::ast::SelectItem::ExprWithAlias { expr, .. } => {
                                    traverse_expr(
                                        expr,
                                        &table_names,
                                        all_tables,
                                        &mut results,
                                        hint,
                                    )?;
                                }
                                _ => {}
                            }
                        }
                        if let Some(selection) = &select.selection {
                            traverse_expr(selection, &table_names, all_tables, &mut results, None)?;
                        }
                        traverse_returning(
                            &insert_node.returning,
                            &table_names,
                            all_tables,
                            &mut results,
                        )?;
                    }
                    _ => {
                        traverse_query(source_query, &table_names, all_tables, &mut results)?;
                    }
                }
            }
            if let Some(on_insert) = &insert_node.on
                && let sqlparser::ast::OnInsert::OnConflict(on_conflict) = on_insert
                && let sqlparser::ast::OnConflictAction::DoUpdate(do_update) = &on_conflict.action
            {
                for assignment in &do_update.assignments {
                    let col_name = match &assignment.target {
                        sqlparser::ast::AssignmentTarget::ColumnName(obj_name) => {
                            obj_name.0.last().map(|p| p.to_string()).unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    let mut hint = None;
                    if !col_name.is_empty()
                        && let Some(cols) = all_tables.get(&t_name)
                        && let Some(col_info) = cols.iter().find(|c| c.name == col_name)
                    {
                        hint = Some(col_info.data_type.clone());
                    }
                    traverse_expr(
                        &assignment.value,
                        &table_names,
                        all_tables,
                        &mut results,
                        hint,
                    )
                    .map_err(|mut e| {
                        e.message =
                            format!("{} ON CONFLICT assignment to '{}'", e.message, col_name);
                        e
                    })?;
                }
                if let Some(selection) = &do_update.selection {
                    traverse_expr(
                        selection,
                        &table_names,
                        all_tables,
                        &mut results,
                        bool_hint.clone(),
                    )?;
                }
            }
            traverse_returning(
                &insert_node.returning,
                &table_names,
                all_tables,
                &mut results,
            )?;
        }
        _ => {}
    }
    Ok(results)
}

fn traverse_expr(
    expr: &Expr,
    table_names: &Vec<String>,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
    results: &mut Vec<Type>,
    parent_hint: Option<Type>,
) -> Result<(), InferenceError> {
    match expr {
        Expr::Subquery(query) => {
            traverse_query(query, table_names, all_tables, results)?;
            Ok(())
        }

        Expr::Exists { subquery, .. } => {
            traverse_query(subquery, table_names, all_tables, results)?;
            Ok(())
        }

        Expr::InSubquery {
            expr: inner,
            subquery,
            ..
        } => {
            traverse_expr(inner, table_names, all_tables, results, None)?;
            traverse_query(subquery, table_names, all_tables, results)?;
            Ok(())
        }

        Expr::Value(val) => {
            if let Some(hint) = &parent_hint
                && hint.base_type == BaseType::Bool
            {
                match &val.value {
                    sqlparser::ast::Value::Number(n, _) if n != "0" && n != "1" => {
                        return Err(err_from_expr(
                            expr,
                            format!(
                                "Literal '{}' violates boolean check constraint (must be 0 or 1). Alternatively, use keywords TRUE or FALSE",
                                n
                            ),
                        ));
                    }

                    sqlparser::ast::Value::SingleQuotedString(s)
                    | sqlparser::ast::Value::DoubleQuotedString(s) => {
                        return Err(err_from_expr(
                            expr,
                            format!(
                                "String '{}' cannot be inserted into Boolean column. Use 0 or 1. Alternatively, use keywords TRUE or FALSE",
                                s
                            ),
                        ));
                    }

                    sqlparser::ast::Value::Boolean(_) => {}

                    sqlparser::ast::Value::Null => {}

                    sqlparser::ast::Value::Placeholder(_) => {}
                    _ => {}
                }
            }

            if let sqlparser::ast::Value::Placeholder(_) = val.value {
                let t = parent_hint.ok_or_else(|| {
                    err_from_expr(
                        expr,
                        "Unable to infer type. Consider casting with `::` or `CAST AS`",
                    )
                })?;

                if t.base_type == BaseType::PlaceHolder || t.base_type == BaseType::Unknowns {
                    return Err(err_from_expr(
                        expr,
                        "Unable to infer type. Consider casting with `::` or `CAST AS`",
                    ));
                }

                results.push(t);
            }
            Ok(())
        }
        Expr::Like {
            expr: target,
            pattern,
            escape_char,
            ..
        } => {
            traverse_expr(
                target,
                table_names,
                all_tables,
                results,
                Some(Type {
                    base_type: BaseType::Text,
                    nullable: true,
                    contains_placeholder: false,
                }),
            )
            .map_err(|mut e| {
                e.message = format!("{} in LIKE target expression", e.message);
                e
            })?;

            traverse_expr(
                pattern,
                table_names,
                all_tables,
                results,
                Some(Type {
                    base_type: BaseType::Text,
                    nullable: true,
                    contains_placeholder: false,
                }),
            )
            .map_err(|mut e| {
                e.message = format!("{} in LIKE pattern", e.message);
                e
            })?;

            if let Some(escape) = escape_char {
                let escape_expr = Expr::Value(escape.clone().into());
                traverse_expr(
                    &escape_expr,
                    table_names,
                    all_tables,
                    results,
                    Some(Type {
                        base_type: BaseType::Text,
                        nullable: true,
                        contains_placeholder: false,
                    }),
                )
                .map_err(|mut e| {
                    e.message = format!("{} in LIKE escape char", e.message);
                    e
                })?;
            }
            Ok(())
        }

        Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            traverse_expr(inner, table_names, all_tables, results, None)?;
            Ok(())
        }

        Expr::IsTrue(inner)
        | Expr::IsFalse(inner)
        | Expr::IsNotTrue(inner)
        | Expr::IsNotFalse(inner) => {
            traverse_expr(
                inner,
                table_names,
                all_tables,
                results,
                Some(Type {
                    base_type: BaseType::Bool,
                    nullable: false,
                    contains_placeholder: false,
                }),
            )?;
            Ok(())
        }

        Expr::UnaryOp { op, expr: inner } => {
            let child_hint = match op {
                sqlparser::ast::UnaryOperator::Not => Some(Type {
                    base_type: BaseType::Bool,
                    nullable: true,
                    contains_placeholder: false,
                }),
                _ => parent_hint,
            };

            traverse_expr(inner, table_names, all_tables, results, child_hint)?;
            Ok(())
        }

        Expr::BinaryOp { left, right, op } => {
            let left_type = evaluate_expr_type(left, table_names, all_tables)
                .map_err(|e| err_from_expr(left.as_ref(), e))?;

            let right_type = evaluate_expr_type(right, table_names, all_tables)
                .map_err(|e| err_from_expr(right.as_ref(), e))?;

            let (left_hint, right_hint) = match op {
                BinaryOperator::Eq
                | BinaryOperator::Gt
                | BinaryOperator::Lt
                | BinaryOperator::GtEq
                | BinaryOperator::LtEq
                | BinaryOperator::NotEq => (Some(right_type), Some(left_type)),

                BinaryOperator::Plus
                | BinaryOperator::Minus
                | BinaryOperator::Multiply
                | BinaryOperator::Modulo
                | BinaryOperator::Divide => (Some(right_type), Some(left_type)),

                BinaryOperator::StringConcat => {
                    let text_type = Type {
                        base_type: BaseType::Text,
                        nullable: true,
                        contains_placeholder: false,
                    };
                    (Some(text_type.clone()), Some(text_type))
                }

                BinaryOperator::And | BinaryOperator::Or => {
                    let bool_type = Type {
                        base_type: BaseType::Bool,
                        nullable: false,
                        contains_placeholder: false,
                    };
                    (Some(bool_type.clone()), Some(bool_type))
                }

                BinaryOperator::BitwiseOr
                | BinaryOperator::BitwiseAnd
                | BinaryOperator::BitwiseXor => {
                    let int_type = Type {
                        base_type: BaseType::Integer,
                        nullable: true,
                        contains_placeholder: false,
                    };
                    (Some(int_type.clone()), Some(int_type))
                }

                _ => (None, None),
            };

            let parent_span = expr.span();
            let err_mapper = |mut e: InferenceError| {
                e.start = parent_span.start.into();
                e.end = parent_span.end.into();
                e.message = format!("{} in expression '{left} {op} {right}'", e.message);
                e
            };

            traverse_expr(left, table_names, all_tables, results, left_hint).map_err(err_mapper)?;
            traverse_expr(right, table_names, all_tables, results, right_hint)
                .map_err(err_mapper)?;

            Ok(())
        }

        Expr::Nested(inner) => traverse_expr(inner, table_names, all_tables, results, parent_hint),

        Expr::InList {
            expr: match_expr,
            list,
            ..
        } => {
            let mut common_type = evaluate_expr_type(match_expr, table_names, all_tables)
                .ok()
                .filter(|t| {
                    t.base_type != BaseType::PlaceHolder && t.base_type != BaseType::Unknowns
                });

            if common_type.is_none() {
                for item in list {
                    if let Ok(t) = evaluate_expr_type(item, table_names, all_tables)
                        && t.base_type != BaseType::PlaceHolder
                        && t.base_type != BaseType::Unknowns
                    {
                        common_type = Some(t);
                        break;
                    }
                }
            }

            let parent_span = expr.span();
            let err_mapper = |mut e: InferenceError| {
                e.start = parent_span.start.into();
                e.end = parent_span.end.into();
                e.message = format!(
                    "{} '{expr}'. Consider casting with `::` or `CAST AS`",
                    e.message
                );
                e
            };

            traverse_expr(
                match_expr,
                table_names,
                all_tables,
                results,
                common_type.clone(),
            )
            .map_err(err_mapper)?;

            for item in list {
                traverse_expr(item, table_names, all_tables, results, common_type.clone())
                    .map_err(err_mapper)?;
            }

            Ok(())
        }

        Expr::Between {
            expr: match_expr,
            low,
            high,
            ..
        } => {
            let e_type = evaluate_expr_type(match_expr, table_names, all_tables);
            let l_type = evaluate_expr_type(low, table_names, all_tables);
            let h_type = evaluate_expr_type(high, table_names, all_tables);

            let context = if let Ok(t) = &e_type
                && t.base_type != BaseType::PlaceHolder
            {
                Some(t.clone())
            } else if let Ok(t) = &l_type
                && t.base_type != BaseType::PlaceHolder
            {
                Some(t.clone())
            } else if let Ok(t) = &h_type
                && t.base_type != BaseType::PlaceHolder
            {
                Some(t.clone())
            } else {
                None
            };

            traverse_expr(
                match_expr,
                table_names,
                all_tables,
                results,
                context.clone(),
            )?;
            traverse_expr(low, table_names, all_tables, results, context.clone())?;
            traverse_expr(high, table_names, all_tables, results, context)?;
            Ok(())
        }

        Expr::Cast {
            expr: inner,
            data_type,
            ..
        } => {
            let nullable = parent_hint.map(|h| h.nullable).unwrap_or(true);

            let target_type = match data_type {
                DataType::Int(_)
                | DataType::Integer(_)
                | DataType::TinyInt(_)
                | DataType::SmallInt(_)
                | DataType::MediumInt(_)
                | DataType::BigInt(_)
                | DataType::BigIntUnsigned(_)
                | DataType::Int2(_)
                | DataType::Int8(_) => Some(Type {
                    base_type: BaseType::Integer,
                    nullable,
                    contains_placeholder: false,
                }),

                DataType::Character(_)
                | DataType::Varchar(_)
                | DataType::CharVarying(_)
                | DataType::CharacterVarying(_)
                | DataType::Nvarchar(_)
                | DataType::Text
                | DataType::Clob(_) => Some(Type {
                    base_type: BaseType::Text,
                    nullable,
                    contains_placeholder: false,
                }),

                DataType::Real
                | DataType::Double(_)
                | DataType::DoublePrecision
                | DataType::Numeric(_)
                | DataType::Decimal(_)
                | DataType::Float(_) => Some(Type {
                    base_type: BaseType::Real,
                    nullable,
                    contains_placeholder: false,
                }),

                DataType::Boolean | DataType::Bool => Some(Type {
                    base_type: BaseType::Bool,
                    nullable,
                    contains_placeholder: false,
                }),

                _ => None,
            };

            let parent_span = expr.span();
            traverse_expr(inner, table_names, all_tables, results, target_type).map_err(
                |mut e| {
                    e.start = parent_span.start.into();
                    e.end = parent_span.end.into();
                    e.message = format!("{} inside CAST", e.message);
                    e
                },
            )?;
            Ok(())
        }

        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            let mut result_hint = parent_hint.clone();

            if result_hint.is_none()
                && let Some(else_expr) = else_result
                && let Ok(t) = evaluate_expr_type(else_expr, table_names, all_tables)
                && t.base_type != BaseType::PlaceHolder
                && t.base_type != BaseType::Unknowns
            {
                result_hint = Some(t);
            }

            if result_hint.is_none() {
                for cond in conditions {
                    if let Ok(t) = evaluate_expr_type(&cond.result, table_names, all_tables)
                        && t.base_type != BaseType::PlaceHolder
                        && t.base_type != BaseType::Unknowns
                    {
                        result_hint = Some(t);
                        break;
                    }
                }
            }

            let operand_hint = if let Some(op_expr) = operand {
                traverse_expr(op_expr, table_names, all_tables, results, None).map_err(
                    |mut e| {
                        e.message = format!("{} in CASE operand '{}'", e.message, op_expr);
                        e
                    },
                )?;
                evaluate_expr_type(op_expr, table_names, all_tables).ok()
            } else {
                None
            };

            let case_span = expr.span();

            for cond in conditions {
                let when_hint = operand_hint.clone().or(Some(Type {
                    base_type: BaseType::Bool,
                    nullable: false,
                    contains_placeholder: false,
                }));

                traverse_expr(&cond.condition, table_names, all_tables, results, when_hint)
                    .map_err(|mut e| {
                        e.message = format!("{} in 'WHEN {}'", e.message, cond.condition);
                        e
                    })?;

                if let Err(mut e) = traverse_expr(
                    &cond.result,
                    table_names,
                    all_tables,
                    results,
                    result_hint.clone(),
                ) {
                    if result_hint.is_none() {
                        e.start = case_span.start.into();
                        e.end = case_span.end.into();

                        let else_text = match else_result {
                            Some(el) => format!(" ELSE {}", el),
                            None => String::new(),
                        };

                        e.message = format!(
                            "Unable to infer type in 'THEN {}{}. Consider casting with `::` or `CAST AS`'",
                            cond.result, else_text
                        );
                    } else {
                        e.message = format!(
                            "{} in 'THEN {} Consider casting with `::` or `CAST AS`'",
                            e.message, cond.result
                        );
                    }
                    return Err(e);
                }
            }

            if let Some(else_expr) = else_result
                && let Err(mut e) = traverse_expr(
                    else_expr,
                    table_names,
                    all_tables,
                    results,
                    result_hint.clone(),
                )
            {
                if result_hint.is_none() {
                    e.start = case_span.start.into();
                    e.end = case_span.end.into();
                    e.message = format!(
                        "Unable to infer type in 'ELSE {}. Consider casting with `::` or `CAST AS`'",
                        else_expr
                    );
                } else {
                    e.message = format!(
                        "{} in 'ELSE {}. Consider casting with `::` or `CAST AS`'",
                        e.message, else_expr
                    );
                }
                return Err(e);
            }

            Ok(())
        }
        Expr::Function(func) => {
            let name = func.name.to_string().to_uppercase();

            let expects_text = matches!(
                name.as_str(),
                "LOWER"
                    | "UPPER"
                    | "LTRIM"
                    | "RTRIM"
                    | "TRIM"
                    | "REPLACE"
                    | "SUBSTR"
                    | "SUBSTRING"
                    | "CONCAT"
                    | "CONCAT_WS"
                    | "LENGTH"
                    | "OCTET_LENGTH"
                    | "INSTR"
                    | "GLOB"
                    | "LIKE"
                    | "DATE"
                    | "TIME"
                    | "DATETIME"
            );

            let expects_number = matches!(
                name.as_str(),
                "ABS"
                    | "ROUND"
                    | "CEIL"
                    | "FLOOR"
                    | "SIN"
                    | "COS"
                    | "TAN"
                    | "ASIN"
                    | "ACOS"
                    | "ATAN"
                    | "SQRT"
                    | "POW"
                    | "POWER"
                    | "LOG"
                    | "LOG10"
                    | "EXP"
                    | "DEGREES"
                    | "RADIANS"
                    | "SIGN"
                    | "MOD"
            );

            let is_polymorphic = matches!(
                name.as_str(),
                "COALESCE"
                    | "IFNULL"
                    | "MIN"
                    | "MAX"
                    | "SUM"
                    | "LEAD"
                    | "LAG"
                    | "NULLIF"
                    | "FIRST_VALUE"
                    | "LAST_VALUE"
            );

            let parent_span = expr.span();
            let err_mapper = |mut e: InferenceError| {
                e.start = parent_span.start.into();
                e.end = parent_span.end.into();
                e.message = format!(
                    "{} unable to infer expr in function {}. Consider casting with `::` or `CAST AS`.",
                    e.message, name
                );
                e
            };

            if let Some(window_type) = &func.over
                && let sqlparser::ast::WindowType::WindowSpec(window_spec) = window_type
            {
                for expr in &window_spec.partition_by {
                    traverse_expr(expr, table_names, all_tables, results, None)
                        .map_err(err_mapper)?;
                }

                for order in &window_spec.order_by {
                    traverse_expr(&order.expr, table_names, all_tables, results, None)
                        .map_err(err_mapper)?;
                }

                if let Some(window_frame) = &window_spec.window_frame {
                    let int_hint = Some(Type {
                        base_type: BaseType::Integer,
                        nullable: false,
                        contains_placeholder: false,
                    });

                    let mut check_bound =
                        |bound: &sqlparser::ast::WindowFrameBound| -> Result<(), InferenceError> {
                            match bound {
                                sqlparser::ast::WindowFrameBound::Preceding(Some(bound_expr))
                                | sqlparser::ast::WindowFrameBound::Following(Some(bound_expr)) => {
                                    traverse_expr(
                                        bound_expr,
                                        table_names,
                                        all_tables,
                                        results,
                                        int_hint.clone(),
                                    )?;
                                }
                                _ => {}
                            }
                            Ok(())
                        };

                    check_bound(&window_frame.start_bound).map_err(err_mapper)?;

                    if let Some(end_bound) = &window_frame.end_bound {
                        check_bound(end_bound).map_err(err_mapper)?;
                    }
                }
            }

            let arg_hint = if expects_text {
                Some(Type {
                    base_type: BaseType::Text,
                    nullable: true,
                    contains_placeholder: false,
                })
            } else if expects_number {
                Some(Type {
                    base_type: BaseType::Real,
                    nullable: true,
                    contains_placeholder: false,
                })
            } else if is_polymorphic {
                let args = match &func.args {
                    FunctionArguments::List(list) => &list.args,
                    _ => &[][..], // Empty slice if no args
                };

                let sibling_hint = args.iter().find_map(|arg| {
                    let FunctionArg::Unnamed(FunctionArgExpr::Expr(arg_expr)) = arg else {
                        return None;
                    };

                    evaluate_expr_type(arg_expr, table_names, all_tables)
                        .ok()
                        .filter(|t| t.base_type != BaseType::PlaceHolder)
                });

                sibling_hint.or(parent_hint)
            } else {
                None
            };

            let parent_span = expr.span();
            let err_mapper = |mut e: InferenceError| {
                e.start = parent_span.start.into();
                e.end = parent_span.end.into();
                e.message = format!(
                    "{} unable to infer expr in function {}. Consider casting with `::` or `CAST AS`.",
                    e.message, name
                );
                e
            };

            if let FunctionArguments::List(args_list) = &func.args {
                for arg in &args_list.args {
                    if let FunctionArg::Unnamed(FunctionArgExpr::Expr(arg_expr)) = arg {
                        traverse_expr(arg_expr, table_names, all_tables, results, arg_hint.clone())
                            .map_err(err_mapper)?;
                    }
                }
            }
            Ok(())
        }

        Expr::Ceil { expr: inner, .. } | Expr::Floor { expr: inner, .. } => {
            let hint = Some(Type {
                base_type: BaseType::Real,
                nullable: true,
                contains_placeholder: false,
            });
            traverse_expr(inner, table_names, all_tables, results, hint)?;
            Ok(())
        }

        Expr::Substring {
            expr: inner,
            substring_from,
            substring_for,
            ..
        } => {
            traverse_expr(
                inner,
                table_names,
                all_tables,
                results,
                Some(Type {
                    base_type: BaseType::Text,
                    nullable: true,
                    contains_placeholder: false,
                }),
            )?;

            if let Some(from_expr) = substring_from {
                traverse_expr(
                    from_expr,
                    table_names,
                    all_tables,
                    results,
                    Some(Type {
                        base_type: BaseType::Integer,
                        nullable: true,
                        contains_placeholder: false,
                    }),
                )?;
            }

            if let Some(for_expr) = substring_for {
                traverse_expr(
                    for_expr,
                    table_names,
                    all_tables,
                    results,
                    Some(Type {
                        base_type: BaseType::Integer,
                        nullable: true,
                        contains_placeholder: false,
                    }),
                )?;
            }
            Ok(())
        }

        Expr::Trim {
            expr: inner,
            trim_what,
            ..
        } => {
            let text_hint = Some(Type {
                base_type: BaseType::Text,
                nullable: true,
                contains_placeholder: false,
            });

            traverse_expr(inner, table_names, all_tables, results, text_hint.clone())?;

            if let Some(pattern_expr) = trim_what {
                traverse_expr(pattern_expr, table_names, all_tables, results, text_hint)?;
            }
            Ok(())
        }

        _ => Ok(()),
    }
}

fn traverse_query(
    query: &sqlparser::ast::Query,
    table_names: &Vec<String>,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
    results: &mut Vec<Type>,
) -> Result<(), InferenceError> {
    let mut local_scope = all_tables.clone();

    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            let inferred_columns = infer_cte_columns(cte, &local_scope);
            let cte_name = cte.alias.name.value.clone();
            local_scope.insert(cte_name, inferred_columns);

            traverse_query(&cte.query, table_names, &local_scope, results)?;
        }
    }

    traverse_set_expr(&query.body, table_names, &local_scope, results)?;

    if let Some(order_by_struct) = &query.order_by
        && let sqlparser::ast::OrderByKind::Expressions(exprs) = &order_by_struct.kind
    {
        for order_expr in exprs {
            traverse_expr(&order_expr.expr, table_names, &local_scope, results, None)?;
        }
    }

    if let Some(limit_clause) = &query.limit_clause {
        let int_hint = Some(Type {
            base_type: BaseType::Integer,
            nullable: false,
            contains_placeholder: false,
        });

        if let sqlparser::ast::LimitClause::LimitOffset { limit, offset, .. } = limit_clause {
            if let Some(limit_expr) = limit {
                traverse_expr(
                    limit_expr,
                    table_names,
                    all_tables,
                    results,
                    int_hint.clone(),
                )?;
            }
            if let Some(offset_struct) = offset {
                traverse_expr(
                    &offset_struct.value,
                    table_names,
                    all_tables,
                    results,
                    int_hint,
                )?;
            }
        }
    }

    Ok(())
}
fn infer_cte_columns(
    cte: &sqlparser::ast::Cte,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Vec<ColumnInfo> {
    let anchor_body = match &*cte.query.body {
        sqlparser::ast::SetExpr::SetOperation { left, .. } => left.as_ref(),
        _ => &cte.query.body,
    };

    if let sqlparser::ast::SetExpr::Select(select) = anchor_body {
        // Clone for CTE alias resolution
        let mut local_tables = all_tables.clone();
        let mut local_scope_names = Vec::new();

        let mut register_table = |relation: &sqlparser::ast::TableFactor| {
            if let sqlparser::ast::TableFactor::Table { name, alias, .. } = relation {
                let real_name = name
                    .0
                    .last()
                    .map(crate::table::normalize_part)
                    .unwrap_or(name.to_string());

                if let Some(cols) = all_tables.get(&real_name) {
                    let effective_name = if let Some(a) = alias {
                        let alias_name = a.name.value.clone();
                        local_tables.insert(alias_name.clone(), cols.clone());
                        alias_name
                    } else {
                        real_name
                    };
                    local_scope_names.push(effective_name);
                }
            }
        };

        for table_with_joins in &select.from {
            register_table(&table_with_joins.relation);
            for join in &table_with_joins.joins {
                register_table(&join.relation);
            }
        }

        let mut cols = Vec::new();
        for (i, item) in select.projection.iter().enumerate() {
            let col_name = if let Some(col_def) = cte.alias.columns.get(i) {
                col_def.name.value.clone()
            } else {
                match item {
                    sqlparser::ast::SelectItem::ExprWithAlias { alias, .. } => alias.value.clone(),
                    sqlparser::ast::SelectItem::UnnamedExpr(sqlparser::ast::Expr::Identifier(
                        ident,
                    )) => ident.value.clone(),
                    sqlparser::ast::SelectItem::UnnamedExpr(
                        sqlparser::ast::Expr::CompoundIdentifier(idents),
                    ) => idents
                        .last()
                        .map(|i| i.value.clone())
                        .unwrap_or_else(|| format!("col_{}", i)),
                    _ => format!("col_{}", i),
                }
            };

            let expr = match item {
                sqlparser::ast::SelectItem::UnnamedExpr(e) => e,
                sqlparser::ast::SelectItem::ExprWithAlias { expr, .. } => expr,
                _ => continue,
            };

            let deduced_type = evaluate_expr_type(expr, &local_scope_names, &local_tables)
                .unwrap_or(Type {
                    base_type: BaseType::Unknowns,
                    nullable: true,
                    contains_placeholder: false,
                });

            cols.push(ColumnInfo {
                name: col_name,
                data_type: deduced_type,
                check_constraint: None,
                has_default: false,
            });
        }
        return cols;
    }
    vec![]
}

#[allow(unused)]
fn traverse_set_expr(
    set_expr: &SetExpr,
    outer_scope: &Vec<String>,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
    results: &mut Vec<Type>,
) -> Result<(), InferenceError> {
    let bool_hint = Some(Type {
        base_type: BaseType::Bool,
        nullable: false,
        contains_placeholder: false,
    });

    match set_expr {
        SetExpr::Select(select) => {
            let mut local_scope_tables = Vec::new();
            let mut current_select_scope = all_tables.clone();

            fn register_table_factor(
                factor: &sqlparser::ast::TableFactor,
                outer_scope: &Vec<String>,
                all_tables: &HashMap<String, Vec<ColumnInfo>>,
                current_select_scope: &mut HashMap<String, Vec<ColumnInfo>>,
                local_scope_tables: &mut Vec<String>,
                results: &mut Vec<Type>,
            ) -> Result<(), InferenceError> {
                match factor {
                    sqlparser::ast::TableFactor::Table { name, alias, .. } => {
                        let real_name = name
                            .0
                            .last()
                            .map(normalize_part)
                            .unwrap_or(name.to_string());
                        let effective_name = if let Some(a) = alias {
                            let alias_name = a.name.value.clone();
                            if let Some(cols) = all_tables.get(&real_name).cloned() {
                                current_select_scope.insert(alias_name.clone(), cols);
                            }
                            alias_name
                        } else {
                            real_name
                        };
                        local_scope_tables.push(effective_name);
                    }
                    sqlparser::ast::TableFactor::Derived {
                        subquery, alias, ..
                    } => {
                        traverse_query(subquery, outer_scope, all_tables, results)?;
                        // If it has an alias, register output columns
                        // of the subquery here, but for param inference, just visiting the subquery is crucial.
                        if let Some(a) = alias {
                            local_scope_tables.push(a.name.value.clone());
                        }
                    }
                    sqlparser::ast::TableFactor::TableFunction { expr, alias, .. } => {
                        traverse_expr(
                            expr,
                            local_scope_tables,
                            current_select_scope,
                            results,
                            None,
                        )?;
                        if let Some(a) = alias {
                            local_scope_tables.push(a.name.value.clone());
                        }
                    }
                    sqlparser::ast::TableFactor::NestedJoin {
                        table_with_joins, ..
                    } => {
                        register_table_factor(
                            &table_with_joins.relation,
                            outer_scope,
                            all_tables,
                            current_select_scope,
                            local_scope_tables,
                            results,
                        )?;
                        for join in &table_with_joins.joins {
                            register_table_factor(
                                &join.relation,
                                outer_scope,
                                all_tables,
                                current_select_scope,
                                local_scope_tables,
                                results,
                            )?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            }

            for table_with_joins in &select.from {
                register_table_factor(
                    &table_with_joins.relation,
                    outer_scope,
                    all_tables,
                    &mut current_select_scope,
                    &mut local_scope_tables,
                    results,
                )?;

                for join in &table_with_joins.joins {
                    register_table_factor(
                        &join.relation,
                        outer_scope,
                        all_tables,
                        &mut current_select_scope,
                        &mut local_scope_tables,
                        results,
                    )?;
                }
            }

            let mut projected_aliases = Vec::new();
            for item in &select.projection {
                match item {
                    sqlparser::ast::SelectItem::UnnamedExpr(expr) => {
                        traverse_expr(
                            expr,
                            &local_scope_tables,
                            &current_select_scope,
                            results,
                            None,
                        )?;
                    }
                    sqlparser::ast::SelectItem::ExprWithAlias { expr, alias } => {
                        traverse_expr(
                            expr,
                            &local_scope_tables,
                            &current_select_scope,
                            results,
                            None,
                        )?;

                        let derived_type =
                            evaluate_expr_type(expr, &local_scope_tables, &current_select_scope)
                                .unwrap_or(Type {
                                    base_type: BaseType::Unknowns,
                                    nullable: true,
                                    contains_placeholder: false,
                                });

                        projected_aliases.push(ColumnInfo {
                            name: alias.value.clone(),
                            data_type: derived_type,
                            check_constraint: None,
                            has_default: false,
                        });
                    }
                    _ => {}
                }
            }

            for table_with_joins in &select.from {
                fn traverse_join_constraints(
                    joins: &[sqlparser::ast::Join],
                    local_scope_tables: &Vec<String>,
                    current_select_scope: &HashMap<String, Vec<ColumnInfo>>,
                    results: &mut Vec<Type>,
                    bool_hint: Option<Type>,
                ) -> Result<(), InferenceError> {
                    for join in joins {
                        let constraint = match &join.join_operator {
                            sqlparser::ast::JoinOperator::Inner(c)
                            | sqlparser::ast::JoinOperator::Left(c)
                            | sqlparser::ast::JoinOperator::LeftOuter(c)
                            | sqlparser::ast::JoinOperator::Right(c)
                            | sqlparser::ast::JoinOperator::RightOuter(c)
                            | sqlparser::ast::JoinOperator::FullOuter(c)
                            | sqlparser::ast::JoinOperator::Join(c) => Some(c),
                            _ => None,
                        };

                        if let Some(sqlparser::ast::JoinConstraint::On(expr)) = constraint {
                            traverse_expr(
                                expr,
                                local_scope_tables,
                                current_select_scope,
                                results,
                                bool_hint.clone(),
                            )?;
                        }

                        // Recurse if the relation itself is a nested join
                        if let sqlparser::ast::TableFactor::NestedJoin {
                            table_with_joins, ..
                        } = &join.relation
                        {
                            traverse_join_constraints(
                                &table_with_joins.joins,
                                local_scope_tables,
                                current_select_scope,
                                results,
                                bool_hint.clone(),
                            )?;
                        }
                    }
                    Ok(())
                }

                traverse_join_constraints(
                    &table_with_joins.joins,
                    &local_scope_tables,
                    &current_select_scope,
                    results,
                    bool_hint.clone(),
                )?;

                if let sqlparser::ast::TableFactor::NestedJoin {
                    table_with_joins: nested,
                    ..
                } = &table_with_joins.relation
                {
                    traverse_join_constraints(
                        &nested.joins,
                        &local_scope_tables,
                        &current_select_scope,
                        results,
                        bool_hint.clone(),
                    )?;
                }
            }

            if let Some(selection) = &select.selection {
                traverse_expr(
                    selection,
                    &local_scope_tables,
                    &current_select_scope,
                    results,
                    bool_hint.clone(),
                )?;
            }

            if !projected_aliases.is_empty() {
                let virtual_alias_table = "$_current_scope_aliases_$".to_string();
                current_select_scope.insert(virtual_alias_table.clone(), projected_aliases);
                local_scope_tables.push(virtual_alias_table);
            }

            if let sqlparser::ast::GroupByExpr::Expressions(exprs, ..) = &select.group_by {
                for expr in exprs {
                    traverse_expr(
                        expr,
                        &local_scope_tables,
                        &current_select_scope,
                        results,
                        None,
                    )?;
                }
            }

            if let Some(having) = &select.having {
                traverse_expr(
                    having,
                    &local_scope_tables,
                    &current_select_scope,
                    results,
                    bool_hint.clone(),
                )?;
            }
        }

        SetExpr::SetOperation { left, right, .. } => {
            traverse_set_expr(left, outer_scope, all_tables, results)?;
            traverse_set_expr(right, outer_scope, all_tables, results)?;
        }

        SetExpr::Query(q) => {
            traverse_query(q, outer_scope, all_tables, results)?;
        }

        SetExpr::Values(values) => {
            for row in &values.rows {
                for expr in row {
                    traverse_expr(expr, outer_scope, all_tables, results, None)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn traverse_returning(
    returning: &Option<Vec<sqlparser::ast::SelectItem>>,
    table_names: &Vec<String>,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
    results: &mut Vec<Type>,
) -> Result<(), InferenceError> {
    if let Some(items) = returning {
        for item in items {
            match item {
                // e.g. RETURNING col + ?
                sqlparser::ast::SelectItem::UnnamedExpr(expr) => {
                    traverse_expr(expr, table_names, all_tables, results, None)?;
                }
                // RETURNING col + ? AS new_val
                sqlparser::ast::SelectItem::ExprWithAlias { expr, .. } => {
                    traverse_expr(expr, table_names, all_tables, results, None)?;
                }
                // RETURNING *
                _ => {}
            }
        }
    }
    Ok(())
}
