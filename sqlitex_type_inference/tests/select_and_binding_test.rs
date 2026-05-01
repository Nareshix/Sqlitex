use sqlitex_type_inference::select_patterns::get_types_from_select;
use sqlitex_type_inference::*;

use crate::{
    binding_patterns::get_type_of_binding_parameters,
    expr::BaseType,
    expr::Type,
    table::{ColumnInfo, create_tables},
};
use std::collections::HashMap;

fn setup_tables() -> HashMap<String, Vec<ColumnInfo>> {
    let mut tables = HashMap::new();
    create_tables(
        "CREATE TABLE users (id INTEGER, name TEXT, costs REAL, age INTEGER)",
        &mut tables,
    );
    create_tables(
        "CREATE TABLE products (product_id INTEGER, price REAL, stock INTEGER, description TEXT)",
        &mut tables,
    );
    create_tables(
        "CREATE TABLE orders (order_id INTEGER, user_id INTEGER, total REAL, created_at TEXT)",
        &mut tables,
    );
    tables
}

fn check_types(sql: &str, expected: (Vec<Type>, Vec<BaseType>)) {
    let tables = setup_tables();
    let binding_parameter_result = get_type_of_binding_parameters(sql, &tables);

    let select_types: Vec<_> = get_types_from_select(sql, &tables)
        .unwrap()
        .into_iter()
        .map(|c| c.data_type)
        .collect();
    let binding_parameter: Vec<_> = binding_parameter_result
        .unwrap()
        .iter()
        .map(|t| t.base_type)
        .collect();

    assert_eq!((select_types, binding_parameter), expected);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(base: BaseType, nullable: bool) -> Type {
        Type {
            base_type: base,
            nullable,
            contains_placeholder: false,
        }
    }

    #[test]
    fn test_select_simple_columns() {
        check_types(
            "SELECT id, name FROM users",
            (
                vec![t(BaseType::Integer, true), t(BaseType::Text, true)],
                vec![],
            ),
        );
    }

    #[test]
    fn test_select_wildcard() {
        check_types(
            "SELECT * FROM products",
            (
                vec![
                    t(BaseType::Integer, true),
                    t(BaseType::Real, true),
                    t(BaseType::Integer, true),
                    t(BaseType::Text, true),
                ],
                vec![],
            ),
        );
    }

    #[test]
    fn test_basic_where_binding() {
        check_types(
            "SELECT name FROM users WHERE id = ?",
            (vec![t(BaseType::Text, true)], vec![BaseType::Integer]),
        );
    }

    #[test]
    fn test_multiple_where_bindings() {
        check_types(
            "SELECT order_id FROM orders WHERE user_id = ? AND total > ?",
            (
                vec![t(BaseType::Integer, true)],
                vec![BaseType::Integer, BaseType::Real],
            ),
        );
    }

    #[test]
    fn test_insert_inference() {
        check_types(
            "INSERT INTO users (name, age) VALUES (?, ?)",
            (vec![], vec![BaseType::Text, BaseType::Integer]),
        );
    }

    #[test]
    fn test_update_inference() {
        check_types(
            "UPDATE users SET costs = ? WHERE id = ?",
            (vec![], vec![BaseType::Real, BaseType::Integer]),
        );
    }

    #[test]
    fn test_delete_inference() {
        check_types(
            "DELETE FROM products WHERE product_id = ?",
            (vec![], vec![BaseType::Integer]),
        );
    }

    #[test]
    fn test_binary_op_promotion_and_binding() {
        check_types(
            "SELECT costs + 10 FROM users WHERE age > ?",
            (vec![t(BaseType::Real, true)], vec![BaseType::Integer]),
        );
    }

    #[test]
    fn test_like_inference() {
        check_types(
            "SELECT id FROM users WHERE name LIKE ?",
            (vec![t(BaseType::Integer, true)], vec![BaseType::Text]),
        );
    }

    #[test]
    fn test_in_list_inference() {
        check_types(
            "SELECT name FROM users WHERE id IN (?, ?, 5)",
            (
                vec![t(BaseType::Text, true)],
                vec![BaseType::Integer, BaseType::Integer],
            ),
        );
    }

    #[test]
    fn test_aggregates_count() {
        check_types(
            "SELECT COUNT(id) FROM users",
            (vec![t(BaseType::Integer, false)], vec![]),
        );
    }

    #[test]
    fn test_aggregates_avg() {
        check_types(
            "SELECT AVG(age) FROM users",
            (vec![t(BaseType::Real, true)], vec![]),
        );
    }

    #[test]
    fn test_explicit_cast_output() {
        check_types(
            "SELECT CAST(age AS REAL) FROM users",
            (vec![t(BaseType::Real, true)], vec![]),
        );
    }

    #[test]
    fn test_join_column_resolution() {
        check_types(
            "SELECT products.description, orders.total
             FROM products
             JOIN orders ON products.product_id = orders.order_id",
            (
                vec![t(BaseType::Text, true), t(BaseType::Real, true)],
                vec![],
            ),
        );
    }

    #[test]
    fn test_case_statement() {
        check_types(
            "SELECT CASE WHEN costs > ? THEN 'High' ELSE 'Low' END FROM users",
            (vec![t(BaseType::Text, false)], vec![BaseType::Real]),
        );
    }

    #[test]
    fn test_between_inference() {
        check_types(
            "SELECT name FROM users WHERE age BETWEEN ? AND ?",
            (
                vec![t(BaseType::Text, true)],
                vec![BaseType::Integer, BaseType::Integer],
            ),
        );
    }
}
