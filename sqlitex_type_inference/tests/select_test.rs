use pretty_assertions::assert_eq;
use sqlitex_type_inference::select_patterns::get_types_from_select;
use sqlitex_type_inference::*;

use crate::{
    expr::BaseType,
    table::{ColumnInfo, create_tables},
};
use std::collections::HashMap;

#[derive(Debug, PartialEq)]

struct ColumnType {
    base_type: BaseType,
    nullable: bool,
}

fn setup_tables() -> HashMap<String, Vec<ColumnInfo>> {
    let mut tables = HashMap::new();

    create_tables(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, costs REAL, age INTEGER)",
        &mut tables,
    );

    create_tables(
        "CREATE TABLE products (product_id INTEGER NOT NULL, price REAL NOT NULL, stock INTEGER, description TEXT)",
        &mut tables,
    );

    create_tables(
        "CREATE TABLE orders (order_id INTEGER NOT NULL, user_id INTEGER NOT NULL, total REAL NOT NULL, created_at TEXT NOT NULL)",
        &mut tables,
    );
    create_tables(
        "CREATE TABLE flags (
            flag_id INTEGER NOT NULL,
            is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)),
            is_admin INTEGER CHECK (is_admin = 0 OR is_admin = 1),
            is_visible INTEGER NOT NULL CHECK (0 = is_visible OR 1 = is_visible),
            manual_bool BOOL
        )",
        &mut tables,
    );
    tables
}

#[track_caller]
fn check_select_types(sql: &str, expected: Vec<ColumnType>) {
    let tables = setup_tables();

    let internal_types: Vec<_> = get_types_from_select(sql, &tables)
        .unwrap()
        .into_iter()
        .map(|c| c.data_type)
        .collect();

    let select_types: Vec<ColumnType> = internal_types
        .into_iter()
        .map(|t| ColumnType {
            base_type: t.base_type,
            nullable: t.nullable,
        })
        .collect();

    assert_eq!(select_types, expected);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(base: BaseType, nullable: bool) -> ColumnType {
        ColumnType {
            base_type: base,
            nullable,
        }
    }

    #[test]
    fn test_select_simple_columns() {
        check_select_types(
            "SELECT id, name FROM users",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_select_wildcard() {
        check_select_types(
            "SELECT * FROM products",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Real, false),
                t(BaseType::Integer, true),
                t(BaseType::Text, true),
            ],
        );
    }

    #[test]
    fn test_select_literals() {
        check_select_types(
            "SELECT 1, 'hello', 3.14 FROM users",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Text, false),
                t(BaseType::Real, false),
            ],
        );
    }

    #[test]
    fn test_expression_math_not_null() {
        check_select_types(
            "SELECT price * 2 FROM products",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_expression_math_nullable_mix() {
        check_select_types(
            "SELECT stock + 10 FROM products",
            vec![t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_expression_string_concat() {
        check_select_types(
            "SELECT name || ' suffix' FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_expression_string_concat_nullable() {
        check_select_types(
            "SELECT description || ' suffix' FROM products",
            vec![t(BaseType::Text, true)],
        );
    }

    #[test]
    fn test_basic_where_binding() {
        check_select_types(
            "SELECT name FROM users WHERE id = ?",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_multiple_where_bindings() {
        check_select_types(
            "SELECT order_id FROM orders WHERE user_id = ? AND total > ?",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_insert_inference() {
        check_select_types("INSERT INTO users (name, age) VALUES (?, ?)", vec![]);
    }

    #[test]
    fn test_update_inference() {
        check_select_types("UPDATE users SET costs = ? WHERE id = ?", vec![]);
    }

    #[test]
    fn test_delete_inference() {
        check_select_types("DELETE FROM products WHERE product_id = ?", vec![]);
    }

    #[test]
    fn test_binary_op_promotion_and_binding() {
        check_select_types(
            "SELECT costs + 10 FROM users WHERE age > ?",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_complex_math_expression() {
        check_select_types(
            "SELECT price * stock FROM products",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_like_inference() {
        check_select_types(
            "SELECT id FROM users WHERE name LIKE ?",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_in_list_inference() {
        check_select_types(
            "SELECT name FROM users WHERE id IN (?, ?, 5)",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_aggregates_count() {
        check_select_types(
            "SELECT COUNT(id) FROM users",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_aggregates_avg() {
        check_select_types("SELECT AVG(age) FROM users", vec![t(BaseType::Real, true)]);
    }

    #[test]
    fn test_aggregates_sum() {
        check_select_types(
            "SELECT SUM(total) FROM orders",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_explicit_cast_binding() {
        check_select_types("SELECT CAST(? AS TEXT)", vec![t(BaseType::Text, true)]);
    }

    #[test]
    fn test_explicit_cast_output() {
        check_select_types(
            "SELECT CAST(age AS REAL) FROM users",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_explicit_cast_output_not_null() {
        check_select_types(
            "SELECT CAST(id AS REAL) FROM users",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_explicit_cast_binding_pg() {
        check_select_types("SELECT ?::TEXT", vec![t(BaseType::Text, true)]);
    }

    #[test]
    fn test_explicit_cast_output_pg() {
        check_select_types("SELECT age::REAL FROM users", vec![t(BaseType::Real, true)]);
    }

    #[test]
    fn test_explicit_cast_output_not_null_pg() {
        check_select_types("SELECT id::REAL FROM users", vec![t(BaseType::Real, false)]);
    }
    #[test]
    fn test_join_column_resolution() {
        check_select_types(
            "SELECT products.description, orders.total
             FROM products
             JOIN orders ON products.product_id = orders.order_id",
            vec![t(BaseType::Text, true), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_case_statement() {
        check_select_types(
            "SELECT CASE WHEN costs > ? THEN 'High' ELSE 'Low' END FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_case_statement_with_null() {
        check_select_types(
            "SELECT CASE WHEN stock > 0 THEN description ELSE 'No Stock' END FROM products",
            vec![t(BaseType::Text, true)],
        );
    }

    #[test]
    fn test_between_inference() {
        check_select_types(
            "SELECT name FROM users WHERE age BETWEEN ? AND ?",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_aliased_columns_and_tables() {
        check_select_types(
            "SELECT u.name AS user_name, u.age FROM users u",
            vec![t(BaseType::Text, false), t(BaseType::Integer, true)],
        );

        check_select_types(
            "SELECT u.name AS user_name, u.age FROM users u",
            vec![t(BaseType::Text, false), t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_implicit_cross_join_aliases() {
        check_select_types(
            "SELECT u.name, p.price FROM users u, products p",
            vec![t(BaseType::Text, false), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_scalar_subquery() {
        check_select_types(
            "SELECT name, (SELECT COUNT(*) FROM orders WHERE user_id = users.id) as order_count FROM users",
            vec![t(BaseType::Text, false), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_derived_table_subquery() {
        check_select_types(
            "SELECT derived.p_id, derived.calc FROM (SELECT product_id as p_id, price * 1.2 as calc FROM products) derived",
            vec![t(BaseType::Integer, false), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_subquery_in_where_in() {
        check_select_types(
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders)",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_left_join_nullability_propagation() {
        check_select_types(
            "SELECT users.name, orders.total
             FROM users
             LEFT JOIN orders ON users.id = orders.user_id",
            vec![t(BaseType::Text, false), t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_inner_join_preserves_not_null() {
        check_select_types(
            "SELECT users.name, orders.total
             FROM users
             JOIN orders ON users.id = orders.user_id",
            vec![t(BaseType::Text, false), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_simple_cte() {
        check_select_types(
            "WITH recent_orders AS (
                SELECT user_id, total FROM orders WHERE total > 100.0
             )
             SELECT total FROM recent_orders",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_cte_renaming_columns() {
        check_select_types(
            "WITH user_lookup(uid, uname) AS (
                SELECT id, name FROM users
             )
             SELECT uname FROM user_lookup WHERE uid = 1",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_chained_ctes() {
        check_select_types(
            "WITH
             step1 AS (SELECT id, costs FROM users),
             step2 AS (SELECT id, costs * 2 as double_cost FROM step1)
             SELECT double_cost FROM step2",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_union_nullability_mixing() {
        check_select_types(
            "SELECT id FROM users UNION SELECT stock FROM products",
            vec![t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_union_type_coercion() {
        check_select_types(
            "SELECT id FROM users UNION SELECT 3.5",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_coalesce_makes_nullable_not_null() {
        check_select_types(
            "SELECT COALESCE(costs, 0.0) FROM users",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_nullif_makes_not_null_nullable() {
        check_select_types(
            "SELECT NULLIF(name, 'admin') FROM users",
            vec![t(BaseType::Text, true)],
        );
    }

    #[test]
    fn test_abs_function_propagates_null() {
        check_select_types(
            "SELECT ABS(costs) FROM users",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_string_functions() {
        check_select_types(
            "SELECT UPPER(name) FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_boolean_literals_and_comparison() {
        check_select_types("SELECT id > 0 FROM users", vec![t(BaseType::Bool, false)]);

        check_select_types(
            "SELECT TRUE, FALSE",
            vec![t(BaseType::Bool, false), t(BaseType::Bool, false)],
        );

        check_select_types(
            "SELECT (costs > 0) AND (age < 100) FROM users",
            vec![t(BaseType::Bool, true)],
        );
    }

    #[test]
    fn test_is_null_check() {
        check_select_types(
            "SELECT costs IS NULL FROM users",
            vec![t(BaseType::Bool, false)],
        );
    }

    #[test]
    fn test_exists_subquery() {
        check_select_types(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = 1)",
            vec![t(BaseType::Bool, false)],
        );
    }
    #[test]
    fn test_select_pure_placeholder() {
        check_select_types("SELECT ?", vec![t(BaseType::PlaceHolder, true)]);
    }

    #[test]
    fn test_placeholder_math_inference() {
        check_select_types("SELECT ? + 100", vec![t(BaseType::Integer, true)]);

        check_select_types("SELECT ? + 100.5", vec![t(BaseType::Real, true)]);
    }
    #[test]
    fn test_window_function_row_number() {
        check_select_types(
            "SELECT ROW_NUMBER() OVER (ORDER BY id) FROM users",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_window_function_aggregates() {
        check_select_types(
            "SELECT SUM(total) OVER (PARTITION BY user_id) FROM orders",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_string_length() {
        check_select_types(
            "SELECT LENGTH(name), LENGTH(costs) FROM users",
            vec![t(BaseType::Integer, false), t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_substr() {
        check_select_types(
            "SELECT SUBSTR(name, 1, 3) FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_except_operation() {
        check_select_types(
            "SELECT id FROM users EXCEPT SELECT stock FROM products",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_recursive_cte_counter() {
        check_select_types(
            "WITH RECURSIVE cnt(x) AS (
            SELECT 1
            UNION ALL
            SELECT x+1 FROM cnt WHERE x < 10
         )
         SELECT x FROM cnt",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_mixed_numeric_types() {
        check_select_types("SELECT 10 + 5.5", vec![t(BaseType::Real, false)]);
    }

    #[test]
    fn test_nested_left_join_nullability() {
        check_select_types(
            "SELECT sub.t
         FROM users
         LEFT JOIN (SELECT total as t, user_id FROM orders) sub
         ON users.id = sub.user_id",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_correlated_subquery() {
        check_select_types(
            "SELECT name,
         (SELECT total FROM orders WHERE user_id = u.id LIMIT 1) as last_order_total
         FROM users u",
            vec![t(BaseType::Text, false), t(BaseType::Real, true)],
        );
    }
    #[test]
    fn test_cte_shadows_table_name() {
        check_select_types(
            "WITH users AS (
            SELECT price, stock FROM products
         )
         SELECT price FROM users",
            vec![t(BaseType::Real, false)],
        );
    }
    #[test]
    fn test_deeply_nested_subquery() {
        check_select_types(
            "SELECT * FROM (
            SELECT (
                SELECT product_id FROM products WHERE price > 1000
            ) as expensive_id
        ) tmp",
            vec![t(BaseType::Integer, true)],
        );
    }
    #[test]
    fn test_cte_with_values_clause() {
        check_select_types(
            "WITH const_data(a, b) AS (
            VALUES(1, 2.5)
         )
         SELECT b FROM const_data",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_cte_usage_inside_derived_table() {
        check_select_types(
            "WITH raw_data AS (SELECT id, costs FROM users)
         SELECT d.double_cost
         FROM (SELECT costs * 2 AS double_cost FROM raw_data) d",
            vec![t(BaseType::Real, true)],
        );
    }
    #[test]
    fn test_comparison_operators_all_types() {
        check_select_types(
            "SELECT id < 10, name = 'test', costs <= 5.0 FROM users",
            vec![
                t(BaseType::Bool, false),
                t(BaseType::Bool, false),
                t(BaseType::Bool, true),
            ],
        );
    }

    #[test]
    fn test_not_operator() {
        check_select_types(
            "SELECT NOT (id > 5) FROM users",
            vec![t(BaseType::Bool, false)],
        );
    }

    #[test]
    fn test_or_operator_nullability() {
        check_select_types(
            "SELECT (costs > 0) OR (age < 100) FROM users",
            vec![t(BaseType::Bool, true)],
        );
    }

    #[test]
    fn test_multiple_aggregates() {
        check_select_types(
            "SELECT COUNT(*), AVG(age), SUM(costs), MAX(id), MIN(name) FROM users",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Real, true),
                t(BaseType::Real, true),
                t(BaseType::Integer, true),
                t(BaseType::Text, true),
            ],
        );
    }

    #[test]
    fn test_group_by_with_aggregate() {
        check_select_types(
            "SELECT age, COUNT(*) FROM users GROUP BY age",
            vec![t(BaseType::Integer, true), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_having_clause() {
        check_select_types(
            "SELECT age, COUNT(*) as cnt FROM users GROUP BY age HAVING COUNT(*) > ?",
            vec![t(BaseType::Integer, true), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_order_by_not_in_select() {
        check_select_types(
            "SELECT name FROM users ORDER BY age DESC",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_limit_and_offset() {
        check_select_types(
            "SELECT id FROM users LIMIT ? OFFSET ?",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_distinct() {
        check_select_types(
            "SELECT DISTINCT age FROM users",
            vec![t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_case_with_multiple_when() {
        check_select_types(
            "SELECT CASE
            WHEN age < 18 THEN 'minor'
            WHEN age < 65 THEN 'adult'
            ELSE 'senior'
         END FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_case_without_else() {
        check_select_types(
            "SELECT CASE WHEN id > 10 THEN 'big' END FROM users",
            vec![t(BaseType::Text, true)],
        );
    }

    #[test]
    fn test_nested_case() {
        check_select_types(
            "SELECT CASE
            WHEN age > 18 THEN
                CASE WHEN costs > 100 THEN 'rich adult' ELSE 'adult' END
            ELSE 'young'
         END FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_in_with_subquery() {
        check_select_types(
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > 100)",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_not_in() {
        check_select_types(
            "SELECT name FROM users WHERE id NOT IN (?, ?, ?)",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_multiple_joins() {
        check_select_types(
            "SELECT u.name, o.total, p.price
         FROM users u
         JOIN orders o ON u.id = o.user_id
         JOIN products p ON o.order_id = p.product_id",
            vec![
                t(BaseType::Text, false),
                t(BaseType::Real, false),
                t(BaseType::Real, false),
            ],
        );
    }

    #[test]
    fn test_mixed_join_types() {
        check_select_types(
            "SELECT u.name, o.total, p.description
         FROM users u
         JOIN orders o ON u.id = o.user_id
         LEFT JOIN products p ON o.order_id = p.product_id",
            vec![
                t(BaseType::Text, false),
                t(BaseType::Real, false),
                t(BaseType::Text, true),
            ],
        );
    }

    #[test]
    fn test_right_join_nullability() {
        check_select_types(
            "SELECT users.name, orders.total
         FROM users
         RIGHT JOIN orders ON users.id = orders.user_id",
            vec![t(BaseType::Text, true), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_full_outer_join_nullability() {
        check_select_types(
            "SELECT users.name, orders.total
         FROM users
         FULL OUTER JOIN orders ON users.id = orders.user_id",
            vec![t(BaseType::Text, true), t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_self_join() {
        check_select_types(
            "SELECT u1.name, u2.name FROM users u1 JOIN users u2 ON u1.id = u2.id + 1",
            vec![t(BaseType::Text, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_union_all() {
        check_select_types(
            "SELECT id FROM users UNION ALL SELECT stock FROM products",
            vec![t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_intersect() {
        check_select_types(
            "SELECT id FROM users INTERSECT SELECT user_id FROM orders",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_multiple_set_operations() {
        check_select_types(
            "SELECT id FROM users UNION SELECT user_id FROM orders EXCEPT SELECT product_id FROM products",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_coalesce_with_multiple_args() {
        check_select_types(
            "SELECT COALESCE(costs, age, 0) FROM users",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_coalesce_all_nullable() {
        check_select_types(
            "SELECT COALESCE(costs, stock) FROM users, products",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_ifnull() {
        check_select_types(
            "SELECT IFNULL(costs, 0.0) FROM users",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_nested_subquery_in_select() {
        check_select_types(
            "SELECT name, (SELECT AVG(total) FROM orders WHERE user_id = (SELECT id FROM users LIMIT 1)) FROM users",
            vec![t(BaseType::Text, false), t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_correlated_subquery_2() {
        check_select_types(
            "SELECT name FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id)",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_multiple_ctes_referencing_each_other() {
        check_select_types(
            "WITH
         user_costs AS (SELECT id, costs FROM users WHERE costs > 0),
         high_spenders AS (SELECT id FROM user_costs WHERE costs > 100),
         names AS (SELECT name FROM users WHERE id IN (SELECT id FROM high_spenders))
         SELECT name FROM names",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_window_function_with_frame() {
        check_select_types(
            "SELECT AVG(total) OVER (ORDER BY order_id ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM orders",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_rank_window_functions() {
        check_select_types(
            "SELECT RANK() OVER (ORDER BY total), DENSE_RANK() OVER (ORDER BY total) FROM orders",
            vec![t(BaseType::Integer, false), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_multiple_window_functions() {
        check_select_types(
            "SELECT
            ROW_NUMBER() OVER (ORDER BY id),
            SUM(costs) OVER (PARTITION BY age),
            name
         FROM users",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Real, true),
                t(BaseType::Text, false),
            ],
        );
    }

    #[test]
    fn test_date_functions() {
        check_select_types(
            "SELECT DATE(created_at), TIME(created_at), DATETIME(created_at) FROM orders",
            vec![
                t(BaseType::Text, true),
                t(BaseType::Text, true),
                t(BaseType::Text, true),
            ],
        );
    }

    #[test]
    fn test_round_function() {
        check_select_types(
            "SELECT ROUND(costs, 2), ROUND(price) FROM users, products",
            vec![t(BaseType::Real, true), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_complex_expression_with_parens() {
        check_select_types(
            "SELECT ((costs + 10) * 2) / (age + 1) FROM users",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_modulo_operator() {
        check_select_types(
            "SELECT id % 10 FROM users",
            vec![t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_negative_numbers() {
        check_select_types(
            "SELECT -costs, -age FROM users",
            vec![t(BaseType::Real, true), t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_unary_plus() {
        check_select_types("SELECT +id FROM users", vec![t(BaseType::Integer, false)]);
    }

    #[test]
    fn test_concat_with_null() {
        check_select_types(
            "SELECT name || description FROM users, products",
            vec![t(BaseType::Text, true)],
        );
    }

    #[test]
    fn test_empty_string_literal() {
        check_select_types("SELECT '' FROM users", vec![t(BaseType::Text, false)]);
    }

    #[test]
    fn test_null_literal() {
        check_select_types("SELECT NULL FROM users", vec![t(BaseType::Null, true)]);
    }

    #[test]
    fn test_trim_functions() {
        check_select_types(
            "SELECT TRIM(name), LTRIM(name), RTRIM(name) FROM users",
            vec![
                t(BaseType::Text, false),
                t(BaseType::Text, false),
                t(BaseType::Text, false),
            ],
        );
    }

    #[test]
    fn test_replace_function() {
        check_select_types(
            "SELECT REPLACE(name, 'a', 'b') FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_instr_function() {
        check_select_types(
            "SELECT INSTR(name, 'test') FROM users",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_max_min_functions() {
        check_select_types(
            "SELECT MAX(costs, age), MIN(1.0, 100) FROM users",
            vec![t(BaseType::Real, true), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_zero_division_potential() {
        check_select_types(
            "SELECT total / 0.0 FROM orders",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_bool_constraint_in_list() {
        check_select_types(
            "SELECT is_active FROM flags",
            vec![t(BaseType::Bool, false)],
        );
    }

    #[test]
    fn test_bool_constraint_or_logic() {
        check_select_types("SELECT is_admin FROM flags", vec![t(BaseType::Bool, true)]);
    }

    #[test]
    fn test_bool_constraint_robust_syntax() {
        check_select_types(
            "SELECT is_visible FROM flags",
            vec![t(BaseType::Bool, false)],
        );
    }

    #[test]
    fn test_explicit_bool_keyword() {
        check_select_types(
            "SELECT manual_bool FROM flags",
            vec![t(BaseType::Bool, true)],
        );
    }

    #[test]
    fn test_bool_inference_in_expressions() {
        check_select_types(
            "SELECT is_active AND is_visible FROM flags",
            vec![t(BaseType::Bool, false)],
        );
    }

    #[test]
    fn test_bool_mixed_with_literals() {
        check_select_types(
            "SELECT is_admin = TRUE FROM flags",
            vec![t(BaseType::Bool, true)],
        );
    }
    #[test]
    fn test_column_alias_basic() {
        check_select_types(
            "SELECT id AS user_id, name AS full_name FROM users",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_table_alias_basic() {
        check_select_types(
            "SELECT u.id, u.name FROM users AS u",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_table_alias_without_as() {
        check_select_types(
            "SELECT u.id, u.name FROM users u",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_expression_alias() {
        check_select_types(
            "SELECT costs * 2 AS double_cost, age + 1 AS next_age FROM users",
            vec![t(BaseType::Real, true), t(BaseType::Integer, true)],
        );
    }

    #[test]
    fn test_aggregate_alias() {
        check_select_types(
            "SELECT COUNT(*) AS total_count, AVG(age) AS avg_age FROM users",
            vec![t(BaseType::Integer, false), t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_subquery_uses_outer_alias() {
        check_select_types(
            "SELECT u.name, (SELECT COUNT(*) FROM orders WHERE user_id = u.id) AS order_count FROM users u",
            vec![t(BaseType::Text, false), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_join_with_aliases() {
        check_select_types(
            "SELECT u.name, o.total
         FROM users AS u
         JOIN orders AS o ON u.id = o.user_id",
            vec![t(BaseType::Text, false), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_join_mixed_alias_styles() {
        check_select_types(
            "SELECT u.name, o.total, p.price
         FROM users u
         JOIN orders AS o ON u.id = o.user_id
         JOIN products p ON o.order_id = p.product_id",
            vec![
                t(BaseType::Text, false),
                t(BaseType::Real, false),
                t(BaseType::Real, false),
            ],
        );
    }

    #[test]
    fn test_self_join_requires_aliases() {
        check_select_types(
            "SELECT u1.name AS name1, u2.name AS name2
         FROM users u1
         JOIN users u2 ON u1.id = u2.id + 1",
            vec![t(BaseType::Text, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_derived_table_alias() {
        check_select_types(
            "SELECT sub.calc FROM (SELECT price * 2 AS calc FROM products) AS sub",
            vec![t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_derived_table_column_reference() {
        check_select_types(
            "SELECT derived.product_id, derived.doubled
         FROM (SELECT product_id, price * 2 AS doubled FROM products) AS derived",
            vec![t(BaseType::Integer, false), t(BaseType::Real, false)],
        );
    }

    #[test]
    fn test_cte_alias_basic() {
        check_select_types(
            "WITH user_data AS (SELECT id, name FROM users)
         SELECT id, name FROM user_data",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_cte_column_aliases() {
        check_select_types(
            "WITH user_data(user_id, user_name) AS (SELECT id, name FROM users)
         SELECT user_id, user_name FROM user_data",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_cte_referenced_by_alias() {
        check_select_types(
            "WITH user_data AS (SELECT id, name FROM users)
         SELECT ud.id, ud.name FROM user_data AS ud",
            vec![t(BaseType::Integer, false), t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_multiple_ctes_with_aliases() {
        check_select_types(
            "WITH
         high_cost AS (SELECT id AS user_id, costs FROM users WHERE costs > 100),
         summary AS (SELECT user_id, costs * 2 AS double_cost FROM high_cost)
         SELECT s.user_id, s.double_cost FROM summary AS s",
            vec![t(BaseType::Integer, false), t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_alias_in_where_clause() {
        check_select_types(
            "SELECT costs AS c FROM users u WHERE u.costs > 100",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_alias_in_having_clause() {
        check_select_types(
            "SELECT age, COUNT(*) AS cnt FROM users GROUP BY age HAVING cnt > 1",
            vec![t(BaseType::Integer, true), t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_alias_in_order_by() {
        check_select_types(
            "SELECT costs * 2 AS double_cost FROM users ORDER BY double_cost",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_window_function_alias() {
        check_select_types(
            "SELECT
            id,
            ROW_NUMBER() OVER (ORDER BY id) AS row_num,
            SUM(costs) OVER (PARTITION BY age) AS total_by_age
         FROM users",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Integer, false),
                t(BaseType::Real, true),
            ],
        );
    }

    #[test]
    fn test_union_with_aliases() {
        check_select_types(
            "SELECT id AS identifier FROM users
         UNION
         SELECT product_id AS identifier FROM products",
            vec![t(BaseType::Integer, false)],
        );
    }

    #[test]
    fn test_case_with_alias() {
        check_select_types(
            "SELECT CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END AS age_group FROM users",
            vec![t(BaseType::Text, false)],
        );
    }

    #[test]
    fn test_nested_subquery_aliases() {
        check_select_types(
            "SELECT outer_sub.total_spent
         FROM (
             SELECT inner_sub.user_id, SUM(inner_sub.amount) AS total_spent
             FROM (
                 SELECT user_id, total AS amount FROM orders
             ) AS inner_sub
             GROUP BY inner_sub.user_id
         ) AS outer_sub",
            vec![t(BaseType::Real, true)],
        );
    }

    #[test]
    fn test_table_wildcard_with_alias() {
        check_select_types(
            "SELECT u.* FROM users u",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Text, false),
                t(BaseType::Real, true),
                t(BaseType::Integer, true),
            ],
        );
    }

    #[test]
    fn test_multiple_table_wildcards_with_aliases() {
        check_select_types(
            "SELECT u.*, o.order_id, o.total FROM users u JOIN orders o ON u.id = o.user_id",
            vec![
                t(BaseType::Integer, false),
                t(BaseType::Text, false),
                t(BaseType::Real, true),
                t(BaseType::Integer, true),
                t(BaseType::Integer, false),
                t(BaseType::Real, false),
            ],
        );
    }
}
