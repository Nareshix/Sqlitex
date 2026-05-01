use sqlitex_type_inference::*;

use crate::{
    binding_patterns::get_type_of_binding_parameters,
    expr::BaseType,
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

fn check_binding_types(sql: &str, expected: Vec<BaseType>) {
    let tables = setup_tables();
    let binding_parameter_result = get_type_of_binding_parameters(sql, &tables);

    let binding_parameter: Vec<_> = binding_parameter_result
        .unwrap()
        .iter()
        .map(|t| t.base_type)
        .collect();

    assert_eq!(binding_parameter, expected);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_simple_columns() {
        check_binding_types("SELECT id, name FROM users", vec![]);
    }

    #[test]
    fn test_select_wildcard() {
        check_binding_types("SELECT * FROM products", vec![]);
    }

    #[test]
    fn test_basic_where_binding() {
        check_binding_types(
            "SELECT name FROM users WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_multiple_where_bindings() {
        check_binding_types(
            "SELECT order_id FROM orders WHERE user_id = ? AND total > ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_insert_inference() {
        check_binding_types(
            "INSERT INTO users (name, age) VALUES (?, ?)",
            vec![BaseType::Text, BaseType::Integer],
        );
    }

    #[test]
    fn test_update_inference() {
        check_binding_types(
            "UPDATE users SET costs = ? WHERE id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_delete_inference() {
        check_binding_types(
            "DELETE FROM products WHERE product_id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_binary_op_promotion_and_binding() {
        check_binding_types(
            "SELECT costs + 10 FROM users WHERE age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_like_inference() {
        check_binding_types(
            "SELECT id FROM users WHERE name LIKE ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn test_in_list_inference() {
        check_binding_types(
            "SELECT name FROM users WHERE id IN (?, ?, 5)",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_aggregates_count() {
        check_binding_types("SELECT COUNT(id) FROM users", vec![]);
    }

    #[test]
    fn test_aggregates_avg() {
        check_binding_types("SELECT AVG(age) FROM users", vec![]);
    }

    #[test]
    fn test_explicit_cast_binding() {
        check_binding_types("SELECT CAST(? AS TEXT)", vec![BaseType::Text]);
    }

    #[test]
    fn test_explicit_cast_output() {
        check_binding_types("SELECT CAST(age AS REAL) FROM users", vec![]);
    }

    #[test]
    fn test_join_column_resolution() {
        check_binding_types(
            "SELECT products.description, orders.total
             FROM products
             JOIN orders ON products.product_id = orders.order_id",
            vec![],
        );
    }

    #[test]
    fn test_case_statement() {
        check_binding_types(
            "SELECT CASE WHEN costs > ? THEN 'High' ELSE 'Low' END FROM users",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_between_inference() {
        check_binding_types(
            "SELECT name FROM users WHERE age BETWEEN ? AND ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_insert_all_columns() {
        check_binding_types(
            "INSERT INTO products (product_id, price, stock, description) VALUES (?, ?, ?, ?)",
            vec![
                BaseType::Integer,
                BaseType::Real,
                BaseType::Integer,
                BaseType::Text,
            ],
        );
    }

    #[test]
    fn test_insert_multiple_rows() {
        check_binding_types(
            "INSERT INTO users (name, age) VALUES (?, ?), (?, ?)",
            vec![
                BaseType::Text,
                BaseType::Integer,
                BaseType::Text,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn test_insert_reordered_columns() {
        check_binding_types(
            "INSERT INTO orders (total, user_id, order_id) VALUES (?, ?, ?)",
            vec![BaseType::Real, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_update_multiple_columns() {
        check_binding_types(
            "UPDATE users SET name = ?, age = ?, costs = ? WHERE id = ?",
            vec![
                BaseType::Text,
                BaseType::Integer,
                BaseType::Real,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn test_update_with_multiple_where_conditions() {
        check_binding_types(
            "UPDATE products SET price = ? WHERE stock < ? AND description = ?",
            vec![BaseType::Real, BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn test_update_with_expression() {
        check_binding_types(
            "UPDATE products SET price = price * ? WHERE product_id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_delete_with_multiple_conditions() {
        check_binding_types(
            "DELETE FROM orders WHERE user_id = ? AND total < ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_delete_with_or_conditions() {
        check_binding_types(
            "DELETE FROM products WHERE stock = ? OR price > ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_arithmetic_with_binding() {
        check_binding_types("SELECT price * ? FROM products", vec![BaseType::Real]);
    }

    #[test]
    fn test_complex_arithmetic_expression() {
        check_binding_types(
            "SELECT (price * ?) + (stock - ?) FROM products",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_division_type_promotion() {
        check_binding_types(
            "SELECT total / ? FROM orders WHERE user_id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_modulo_operation() {
        check_binding_types(
            "SELECT id FROM users WHERE age % ? = 0",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_like_with_escape() {
        check_binding_types(
            "SELECT name FROM users WHERE name LIKE ? ESCAPE '\\'",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn test_concatenation() {
        check_binding_types("SELECT name || ? FROM users", vec![BaseType::Text]);
    }

    #[test]
    fn test_multiple_string_operations() {
        check_binding_types(
            "SELECT name FROM users WHERE name LIKE ? AND name != ?",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn test_in_all_bindings() {
        check_binding_types(
            "SELECT name FROM users WHERE age IN (?, ?, ?)",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_not_in_with_text() {
        check_binding_types(
            "SELECT product_id FROM products WHERE description NOT IN (?, ?, ?)",
            vec![BaseType::Text, BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn test_in_with_mixed_types() {
        check_binding_types(
            "SELECT order_id FROM orders WHERE total IN (?, 100.5, ?)",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_in_subquery_style() {
        check_binding_types(
            "SELECT name FROM users WHERE id IN (?, ?) AND age > ?",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_between_with_real() {
        check_binding_types(
            "SELECT product_id FROM products WHERE price BETWEEN ? AND ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_not_between() {
        check_binding_types(
            "SELECT order_id FROM orders WHERE total NOT BETWEEN ? AND ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_between_with_dates() {
        check_binding_types(
            "SELECT order_id FROM orders WHERE created_at BETWEEN ? AND ?",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn test_multiple_between_conditions() {
        check_binding_types(
            "SELECT product_id FROM products WHERE price BETWEEN ? AND ? AND stock BETWEEN ? AND ?",
            vec![
                BaseType::Real,
                BaseType::Real,
                BaseType::Integer,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn test_aggregates_with_where() {
        check_binding_types(
            "SELECT AVG(price), COUNT(*) FROM products WHERE stock > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_aggregates_sum_with_group_by() {
        check_binding_types(
            "SELECT user_id, SUM(total) FROM orders WHERE total > ? GROUP BY user_id",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_having_clause() {
        check_binding_types(
            "SELECT user_id, AVG(total) FROM orders GROUP BY user_id HAVING AVG(total) > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_count_with_distinct() {
        check_binding_types(
            "SELECT COUNT(DISTINCT user_id) FROM orders WHERE total > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_min_max_aggregates() {
        check_binding_types(
            "SELECT MIN(price), MAX(price) FROM products WHERE stock > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_case_with_multiple_conditions() {
        check_binding_types(
            "SELECT CASE WHEN age < ? THEN 'Young' WHEN age > ? THEN 'Old' ELSE 'Middle' END FROM users",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    #[should_panic]
    fn test_case_with_binding_in_result() {
        check_binding_types(
            "SELECT CASE WHEN costs > 100 THEN ? ELSE ? END FROM users",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn test_nested_case_statements() {
        check_binding_types(
            "SELECT CASE WHEN age > ? THEN CASE WHEN costs > ? THEN 'A' ELSE 'B' END ELSE 'C' END FROM users",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_case_in_where_clause() {
        check_binding_types(
            "SELECT name FROM users WHERE CASE WHEN age > ? THEN 1 ELSE 0 END = 1",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_cast_binding_to_integer() {
        check_binding_types(
            "SELECT name FROM users WHERE id = CAST(? AS INTEGER)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_cast_binding_to_real() {
        check_binding_types(
            "SELECT product_id FROM products WHERE price > CAST(? AS REAL)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_multiple_casts() {
        check_binding_types(
            "SELECT CAST(? AS TEXT), CAST(? AS INTEGER), CAST(? AS REAL)",
            vec![BaseType::Text, BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_join_with_binding_in_on() {
        check_binding_types(
            "SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE o.total > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_multiple_joins_with_bindings() {
        check_binding_types(
            "SELECT u.name, o.total FROM users u
             JOIN orders o ON u.id = o.user_id
             WHERE u.age > ? AND o.total < ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_left_join_with_bindings() {
        check_binding_types(
            "SELECT u.name FROM users u
             LEFT JOIN orders o ON u.id = o.user_id
             WHERE u.age = ? OR o.total IS NULL",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_all_comparison_operators() {
        check_binding_types(
            "SELECT id FROM users WHERE age >= ? AND costs <= ? AND id != ?",
            vec![BaseType::Integer, BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_null_comparison() {
        check_binding_types(
            "SELECT name FROM users WHERE costs IS NOT NULL AND age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_complex_and_or_logic() {
        check_binding_types(
            "SELECT id FROM users WHERE (age > ? AND costs < ?) OR (name = ? AND id != ?)",
            vec![
                BaseType::Integer,
                BaseType::Real,
                BaseType::Text,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn test_not_operator() {
        check_binding_types(
            "SELECT name FROM users WHERE NOT (age < ? OR costs > ?)",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_subquery_in_where() {
        check_binding_types(
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > ?)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_exists_subquery() {
        check_binding_types(
            "SELECT name FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND o.total > ?)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_subquery_with_multiple_bindings() {
        check_binding_types(
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > ? AND created_at = ?)",
            vec![BaseType::Real, BaseType::Text],
        );
    }

    #[test]
    fn test_limit_with_binding() {
        check_binding_types(
            "SELECT name FROM users WHERE age > ? LIMIT ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_limit_offset_with_bindings() {
        check_binding_types(
            "SELECT name FROM users WHERE age > ? LIMIT ? OFFSET ?",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_order_by_with_where_binding() {
        check_binding_types(
            "SELECT name FROM users WHERE age > ? ORDER BY costs DESC",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_order_by_case_with_binding() {
        check_binding_types(
            "SELECT name FROM users ORDER BY CASE WHEN age > ? THEN 1 ELSE 0 END",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_length_function() {
        check_binding_types(
            "SELECT name FROM users WHERE LENGTH(name) > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_upper_lower_functions() {
        check_binding_types(
            "SELECT UPPER(name) FROM users WHERE LOWER(name) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn test_coalesce_function() {
        check_binding_types(
            "SELECT COALESCE(name, ?) FROM users WHERE age > ?",
            vec![BaseType::Text, BaseType::Integer],
        );
    }

    #[test]
    fn test_nullif_function() {
        check_binding_types(
            "SELECT NULLIF(costs, ?) FROM users WHERE id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_abs_function() {
        check_binding_types(
            "SELECT name FROM users WHERE ABS(costs - ?) < 10",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_round_function() {
        check_binding_types(
            "SELECT ROUND(costs, ?) FROM users WHERE id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_complex_query_multiple_clauses() {
        check_binding_types(
            "SELECT u.name, COUNT(o.order_id)
             FROM users u
             LEFT JOIN orders o ON u.id = o.user_id
             WHERE u.age BETWEEN ? AND ?
             AND (u.costs > ? OR o.total < ?)
             GROUP BY u.id
             HAVING COUNT(o.order_id) > ?
             ORDER BY u.name
             LIMIT ?",
            vec![
                BaseType::Integer,
                BaseType::Integer,
                BaseType::Real,
                BaseType::Real,
                BaseType::Integer,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn test_insert_with_select() {
        check_binding_types(
            "INSERT INTO orders (user_id, total)
             SELECT id, ? FROM users WHERE age > ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_update_with_subquery() {
        check_binding_types(
            "UPDATE users SET costs = ?
             WHERE id IN (SELECT user_id FROM orders WHERE total > ?)",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_multiple_case_and_aggregates() {
        check_binding_types(
            "SELECT
                CASE WHEN AVG(total) > ? THEN 'High' ELSE 'Low' END,
                SUM(CASE WHEN total > ? THEN 1 ELSE 0 END)
             FROM orders
             WHERE user_id = ?",
            vec![BaseType::Real, BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn test_union_queries() {
        check_binding_types(
            "SELECT name FROM users WHERE age > ?
             UNION
             SELECT description FROM products WHERE price < ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_nested_expressions() {
        check_binding_types(
            "SELECT name FROM users
             WHERE ((age + ?) * 2) > (costs - ?)
             AND name LIKE ?",
            vec![BaseType::Integer, BaseType::Real, BaseType::Text],
        );
    }

    #[test]
    fn test_multiple_in_clauses() {
        check_binding_types(
            "SELECT name FROM users
             WHERE age IN (?, ?, ?)
             AND id IN (SELECT user_id FROM orders WHERE total IN (?, ?))",
            vec![
                BaseType::Integer,
                BaseType::Integer,
                BaseType::Integer,
                BaseType::Real,
                BaseType::Real,
            ],
        );
    }

    #[test]
    fn test_string_concatenation_chain() {
        check_binding_types(
            "SELECT name || ? || ? || ? FROM users WHERE id = ?",
            vec![
                BaseType::Text,
                BaseType::Text,
                BaseType::Text,
                BaseType::Integer,
            ],
        );
    }
    #[test]
    fn test_insert_returning() {
        check_binding_types(
            "INSERT INTO users (name, age) VALUES (?, ?) RETURNING id, name",
            vec![BaseType::Text, BaseType::Integer],
        );
    }

    #[test]
    fn test_upsert_on_conflict() {
        check_binding_types(
            "INSERT INTO products (product_id, price) VALUES (?, ?)
         ON CONFLICT(product_id) DO UPDATE SET price = ?",
            vec![BaseType::Integer, BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_cte_common_table_expression() {
        check_binding_types(
            "WITH high_spenders AS (
            SELECT user_id FROM orders WHERE total > ?
         )
         SELECT name FROM users WHERE id IN (SELECT user_id FROM high_spenders)",
            vec![BaseType::Real],
        );
    }
    #[test]
    fn test_sqlite_date_functions() {
        check_binding_types(
            "SELECT id FROM orders WHERE created_at > date(?)",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn test_sqlite_datetime_modifier() {
        check_binding_types("SELECT date('now', ?)", vec![BaseType::Text]);
    }

    #[test]
    fn test_intersect() {
        check_binding_types(
            "SELECT name FROM users WHERE age > ?
         INTERSECT
         SELECT description FROM products WHERE price > ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn test_window_function() {
        check_binding_types(
        "SELECT total,
         AVG(total) OVER (PARTITION BY user_id ORDER BY created_at ROWS BETWEEN ? PRECEDING AND CURRENT ROW)
         FROM orders",
        vec![BaseType::Integer],
    );
    }
    #[test]
    fn test_update_from_clause() {
        check_binding_types(
            "UPDATE users
         SET costs = 100
         FROM products
         WHERE products.product_id = products.price
         AND products.stock > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn test_correlated_subquery() {
        check_binding_types(
            "SELECT name FROM users u1
         WHERE age > (SELECT AVG(age) FROM users u2 WHERE u2.costs > ? AND u2.id != u1.id)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn test_update_with_from_join() {
        check_binding_types(
            "UPDATE users SET costs = ?
         FROM orders
         WHERE users.id = orders.user_id AND orders.total > ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn test_recursive_cte() {
        check_binding_types(
            "WITH RECURSIVE cnt(x) AS (
             SELECT ?::int
             UNION ALL
             SELECT x+1 FROM cnt WHERE x < ?::int
         )
         SELECT x FROM cnt",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn test_multiple_ctes_referencing_each_other() {
        check_binding_types(
        "WITH
         high_orders AS (SELECT user_id FROM orders WHERE total > ?),
         rich_users AS (SELECT id FROM users WHERE id IN (SELECT user_id FROM high_orders) AND costs > ?)
         SELECT name FROM users WHERE id IN (SELECT id FROM rich_users)",
        vec![BaseType::Real, BaseType::Real],
    );
    }
}
