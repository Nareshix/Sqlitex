use sqlparser::{
    ast::{
        Cte, Expr, JoinOperator, SelectItem, SelectItemQualifiedWildcardKind, SetExpr, SetOperator,
        Statement, TableFactor,
    },
    dialect::SQLiteDialect,
    parser::Parser,
};
use std::collections::HashMap;

use crate::table::ColumnInfo;
use crate::{
    expr::{BaseType, evaluate_expr_type},
    table::normalize_identifier,
};
use crate::{pg_cast_syntax_to_sqlite, table::normalize_part};

pub fn get_types_from_select(
    sql: &str,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<Vec<ColumnInfo>, String> {
    let dialect = SQLiteDialect {};
    let sql = pg_cast_syntax_to_sqlite(sql);
    let ast = Parser::parse_sql(&dialect, &sql).map_err(|e| e.to_string())?;

    match &ast[0] {
        Statement::Query(query) => {
            let mut context_tables = all_tables.clone();

            if let Some(with) = &query.with {
                for cte in &with.cte_tables {
                    let cte_name = normalize_identifier(&cte.alias.name);

                    let inferred_cols = resolve_cte_columns(cte, &context_tables)?;

                    let final_cols = inferred_cols
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut col)| {
                            if let Some(alias_def) = cte.alias.columns.get(i) {
                                col.name = normalize_identifier(&alias_def.name);
                            }
                            col
                        })
                        .collect();

                    context_tables.insert(cte_name, final_cols);
                }
            }

            traverse_select_output(&query.body, &context_tables)
        }

        Statement::Insert(insert) => {
            if let Some(returning) = &insert.returning {
                let table_name = match &insert.table {
                    sqlparser::ast::TableObject::TableName(obj_name) => {
                        obj_name.0.last().map(normalize_part).unwrap_or_default()
                    }
                    _ => return Ok(vec![]),
                };

                if table_name.is_empty() {
                    return Err("Invalid table name in INSERT".to_string());
                }

                process_returning_clause(returning, &table_name, all_tables)
            } else {
                Ok(vec![])
            }
        }

        Statement::Update {
            table,
            returning: Some(returning),
            ..
        } => {
            let table_name = extract_name_from_table_factor(&table.relation)
                .ok_or("Could not determine table name for UPDATE")?;
            process_returning_clause(returning, &table_name, all_tables)
        }

        Statement::Delete(delete) => {
            if let Some(returning) = &delete.returning {
                let tables = match &delete.from {
                    sqlparser::ast::FromTable::WithFromKeyword(t) => t,
                    sqlparser::ast::FromTable::WithoutKeyword(t) => t,
                };

                let from_item = tables.first().ok_or("DELETE statement missing table")?;

                let table_name = extract_name_from_table_factor(&from_item.relation)
                    .ok_or("Could not determine table name for DELETE")?;

                process_returning_clause(returning, &table_name, all_tables)
            } else {
                Ok(vec![])
            }
        }
        _ => Ok(vec![]),
    }
}

fn process_returning_clause(
    returning: &[SelectItem],
    table_name: &str,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<Vec<ColumnInfo>, String> {
    let mut working_tables = all_tables.clone();
    let mut local_scope_tables = vec![];

    // Put the target table into scope so 'RETURNING id' or 'RETURNING table.id' works
    if let Some(cols) = all_tables.get(table_name) {
        working_tables.insert(table_name.to_string(), cols.clone());
        local_scope_tables.push(table_name.to_string());
    }

    let mut output_columns = Vec::new();

    for (i, item) in returning.iter().enumerate() {
        match item {
            // RETURNING id AS user_id
            SelectItem::ExprWithAlias { expr, alias } => {
                let t = evaluate_expr_type(expr, &local_scope_tables, &working_tables)?;
                output_columns.push(ColumnInfo {
                    name: normalize_identifier(alias),
                    data_type: t,
                    check_constraint: None,
                    has_default: false,
                });
            }
            // RETURNING id
            SelectItem::UnnamedExpr(expr) => {
                let t = evaluate_expr_type(expr, &local_scope_tables, &working_tables)?;
                fn extract_name(expr: &Expr) -> Option<String> {
                    match expr {
                        Expr::Identifier(ident) => Some(normalize_identifier(ident)),
                        Expr::CompoundIdentifier(idents) => idents.last().map(normalize_identifier),
                        Expr::Cast { expr, .. } => extract_name(expr),
                        Expr::Nested(inner) => extract_name(inner),
                        _ => None,
                    }
                }

                let name = extract_name(expr).unwrap_or_else(|| format!("col_{}", i));
                output_columns.push(ColumnInfo {
                    name,
                    data_type: t,
                    check_constraint: None,
                    has_default: false,
                });
            }
            // RETURNING *
            SelectItem::Wildcard(_) => {
                if let Some(cols) = all_tables.get(table_name) {
                    output_columns.extend(cols.clone());
                }
            }
            // RETURNING users.*
            SelectItem::QualifiedWildcard(kind, _) => {
                if let SelectItemQualifiedWildcardKind::ObjectName(obj_name) = kind {
                    let alias_name = obj_name
                        .0
                        .last()
                        .map(normalize_part)
                        .unwrap_or(obj_name.to_string());

                    if let Some(cols) = working_tables.get(&alias_name) {
                        output_columns.extend(cols.clone());
                    }
                }
            }
        }
    }
    Ok(output_columns)
}

fn extract_name_from_table_factor(relation: &TableFactor) -> Option<String> {
    match relation {
        TableFactor::Table { name, alias, .. } => {
            if let Some(a) = alias {
                Some(normalize_identifier(&a.name))
            } else {
                name.0.last().map(normalize_part)
            }
        }
        _ => None,
    }
}
pub fn traverse_select_output(
    body: &SetExpr,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<Vec<ColumnInfo>, String> {
    match body {
        SetExpr::Select(select) => {
            let mut local_scope_tables = Vec::new();
            let mut working_tables = all_tables.clone();

            for table_with_joins in &select.from {
                resolve_table_factor(
                    &table_with_joins.relation,
                    false,
                    all_tables,
                    &mut working_tables,
                    &mut local_scope_tables,
                )?;

                for join in &table_with_joins.joins {
                    // Current table (Right side of the join syntax) becomes nullable
                    // if it is a LEFT or FULL join.
                    let make_current_table_nullable = matches!(
                        &join.join_operator,
                        JoinOperator::Left(_)
                            | JoinOperator::LeftOuter(_)
                            | JoinOperator::FullOuter(_)
                    );

                    // Existing tables (Left side of the join syntax) become nullable
                    // if it is a RIGHT or FULL join.
                    let make_existing_tables_nullable = matches!(
                        &join.join_operator,
                        JoinOperator::Right(_)
                            | JoinOperator::RightOuter(_)
                            | JoinOperator::FullOuter(_)
                    );

                    if make_existing_tables_nullable {
                        for table_name in &local_scope_tables {
                            if let Some(cols) = working_tables.get_mut(table_name) {
                                for col in cols {
                                    col.data_type.nullable = true;
                                }
                            }
                        }
                    }

                    resolve_table_factor(
                        &join.relation,
                        make_current_table_nullable,
                        all_tables,
                        &mut working_tables,
                        &mut local_scope_tables,
                    )?;
                }
            }
            let mut output_columns = Vec::new();

            for (i, item) in select.projection.iter().enumerate() {
                match item {
                    SelectItem::ExprWithAlias { expr, alias } => {
                        let t = evaluate_expr_type(expr, &local_scope_tables, &working_tables)?;
                        output_columns.push(ColumnInfo {
                            name: normalize_identifier(alias),
                            data_type: t,
                            check_constraint: None,
                            has_default: false,
                        });
                    }
                    SelectItem::UnnamedExpr(expr) => {
                        let t = evaluate_expr_type(expr, &local_scope_tables, &working_tables)?;
                        fn extract_name(expr: &Expr) -> Option<String> {
                            match expr {
                                Expr::Identifier(ident) => Some(normalize_identifier(ident)),
                                Expr::CompoundIdentifier(idents) => {
                                    idents.last().map(normalize_identifier)
                                }
                                Expr::Cast { expr, .. } => extract_name(expr),
                                Expr::Nested(inner) => extract_name(inner),
                                _ => None,
                            }
                        }

                        let name = extract_name(expr).unwrap_or_else(|| format!("col_{}", i));
                        output_columns.push(ColumnInfo {
                            name,
                            data_type: t,
                            check_constraint: None,
                            has_default: false,
                        });
                    }
                    SelectItem::Wildcard(_) => {
                        for table_name in &local_scope_tables {
                            if let Some(column_infos) = working_tables.get(table_name) {
                                for column_info in column_infos {
                                    output_columns.push(column_info.clone());
                                }
                            }
                        }
                    }
                    SelectItem::QualifiedWildcard(kind, _) => {
                        if let SelectItemQualifiedWildcardKind::ObjectName(obj_name) = kind {
                            let alias_name = obj_name
                                .0
                                .last()
                                .map(normalize_part)
                                .unwrap_or(obj_name.to_string());

                            if let Some(column_infos) = working_tables.get(&alias_name) {
                                for column_info in column_infos {
                                    output_columns.push(column_info.clone());
                                }
                            }
                        }
                    }
                }
            }
            Ok(output_columns)
        }

        SetExpr::SetOperation {
            op, left, right, ..
        } => {
            let left_cols = traverse_select_output(left, all_tables)?;

            if matches!(op, SetOperator::Union) {
                let right_cols = traverse_select_output(right, all_tables)?;

                let merged_cols = left_cols
                    .into_iter()
                    .zip(right_cols)
                    .map(|(mut l_col, r_col)| {
                        if r_col.data_type.nullable {
                            l_col.data_type.nullable = true;
                        }

                        let l_base = l_col.data_type.base_type;
                        let r_base = r_col.data_type.base_type;

                        if l_base != r_base
                            && ((l_base == BaseType::Integer && r_base == BaseType::Real)
                                || (l_base == BaseType::Real && r_base == BaseType::Integer))
                        {
                            l_col.data_type.base_type = BaseType::Real;
                        }
                        l_col
                    })
                    .collect();

                Ok(merged_cols)
            } else {
                Ok(left_cols)
            }
        }
        SetExpr::Values(values) => {
            let rows = &values.rows;
            if rows.is_empty() {
                return Ok(vec![]);
            }

            let mut final_cols = Vec::new();
            for (i, expr) in rows[0].iter().enumerate() {
                let t = evaluate_expr_type(expr, &vec![], all_tables)?;
                final_cols.push(ColumnInfo {
                    name: format!("col_{}", i),
                    data_type: t,
                    check_constraint: None,
                    has_default: false,
                });
            }

            for row in rows.iter().skip(1) {
                if row.len() != final_cols.len() {
                    return Err("VALUES clause has rows of different lengths".to_string());
                }

                for (i, expr) in row.iter().enumerate() {
                    let new_type = evaluate_expr_type(expr, &vec![], all_tables)?;

                    if new_type.nullable {
                        final_cols[i].data_type.nullable = true;
                    }

                    let l_base = final_cols[i].data_type.base_type;
                    let r_base = new_type.base_type;

                    if l_base != r_base
                        && ((l_base == BaseType::Integer && r_base == BaseType::Real)
                            || (l_base == BaseType::Real && r_base == BaseType::Integer))
                    {
                        final_cols[i].data_type.base_type = BaseType::Real;
                    }
                }
            }
            Ok(final_cols)
        }
        SetExpr::Query(q) => traverse_select_output(&q.body, all_tables),

        _ => Ok(vec![]),
    }
}

fn resolve_cte_columns(
    cte: &Cte,
    context: &HashMap<String, Vec<ColumnInfo>>,
) -> Result<Vec<ColumnInfo>, String> {
    if let SetExpr::SetOperation { left, right, .. } = &*cte.query.body {
        let anchor_cols = traverse_select_output(left, context)?;

        let mut recursive_context = context.clone();
        let cte_name = normalize_identifier(&cte.alias.name);
        let mut context_anchor_cols = Vec::new();
        for (i, col) in anchor_cols.iter().enumerate() {
            let mut new_col = col.clone();
            if let Some(alias_def) = cte.alias.columns.get(i) {
                new_col.name = normalize_identifier(&alias_def.name);
            }
            context_anchor_cols.push(new_col);
        }
        recursive_context.insert(cte_name, context_anchor_cols);

        let recursive_cols = traverse_select_output(right, &recursive_context)?;

        // zip the anchor columns with the recursive columns.
        // If lengths differ, zip stops at the shorter one (standard SQL behavior usually implies they match).
        let merged_cols = anchor_cols
            .into_iter()
            .zip(recursive_cols)
            .map(|(mut l_col, r_col)| {
                if r_col.data_type.nullable {
                    l_col.data_type.nullable = true;
                }

                //  Numeric Types promotion (Int + Real => Real)
                let l_base = l_col.data_type.base_type;
                let r_base = r_col.data_type.base_type;

                if (l_base == BaseType::Integer && r_base == BaseType::Real)
                    || (l_base == BaseType::Real && r_base == BaseType::Integer)
                {
                    l_col.data_type.base_type = BaseType::Real;
                }

                l_col
            })
            .collect();
        Ok(merged_cols)
    } else {
        // Standard CTE
        traverse_select_output(&cte.query.body, context)
    }
}

fn resolve_table_factor(
    relation: &TableFactor,
    force_nullable: bool,
    all_tables: &HashMap<String, Vec<ColumnInfo>>,
    working_tables: &mut HashMap<String, Vec<ColumnInfo>>,
    local_scope_tables: &mut Vec<String>,
) -> Result<(), String> {
    match relation {
        TableFactor::Table { name, alias, .. } => {
            let lookup_name = name
                .0
                .last()
                .map(normalize_part)
                .ok_or("Table name is empty")?;

            let target_alias = if let Some(alias_node) = alias {
                normalize_identifier(&alias_node.name)
            } else {
                lookup_name.clone()
            };

            if let Some(cols) = all_tables.get(&lookup_name) {
                let mut cols = cols.clone();
                if force_nullable {
                    for c in &mut cols {
                        c.data_type.nullable = true;
                    }
                }
                working_tables.insert(target_alias.clone(), cols.clone());
                local_scope_tables.push(target_alias);
            }
        }

        TableFactor::Derived {
            subquery,
            alias: Some(alias_node),
            ..
        } => {
            let alias_name = normalize_identifier(&alias_node.name);
            let mut sub_cols = traverse_select_output(&subquery.body, all_tables)?;

            if force_nullable {
                for c in &mut sub_cols {
                    c.data_type.nullable = true;
                }
            }

            working_tables.insert(alias_name.clone(), sub_cols);
            local_scope_tables.push(alias_name);
        }

        TableFactor::NestedJoin {
            table_with_joins, ..
        } => {
            let scope_start_index = local_scope_tables.len();

            resolve_table_factor(
                &table_with_joins.relation,
                force_nullable,
                all_tables,
                working_tables,
                local_scope_tables,
            )?;

            for join in &table_with_joins.joins {
                let op = &join.join_operator;

                let make_current_table_nullable = matches!(
                    op,
                    JoinOperator::Left(_) | JoinOperator::LeftOuter(_) | JoinOperator::FullOuter(_)
                );

                let make_existing_tables_nullable = matches!(
                    op,
                    JoinOperator::Right(_)
                        | JoinOperator::RightOuter(_)
                        | JoinOperator::FullOuter(_)
                );

                if make_existing_tables_nullable {
                    for table_name in &local_scope_tables[scope_start_index..] {
                        if let Some(cols) = working_tables.get_mut(table_name) {
                            for col in cols {
                                col.data_type.nullable = true;
                            }
                        }
                    }
                }

                resolve_table_factor(
                    &join.relation,
                    force_nullable || make_current_table_nullable,
                    all_tables,
                    working_tables,
                    local_scope_tables,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}
