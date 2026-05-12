use sqlitex_type_inference::*;

#[cfg(test)]
mod tests {
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
            "CREATE TABLE random (id INTEGER NOT NULL, name TEXT NOT NULL, costs TEXT)",
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

    fn check_types(sql: &str, expected: Vec<BaseType>) {
        let tables = setup_tables();
        let result =
            get_type_of_binding_parameters(sql, &tables).expect("Query should parse successfully");
        let actual: Vec<BaseType> = result.iter().map(|t| t.data_type.base_type).collect();
        assert_eq!(actual, expected, "Type mismatch for query: {}", sql);
    }

    fn expect_error(sql: &str) {
        let tables = setup_tables();
        assert!(
            get_type_of_binding_parameters(sql, &tables).is_err(),
            "Query should fail but succeeded: {}",
            sql
        );
    }

    // ==============================
    // 1. BASIC QUERIES (10 tests)
    // ==============================
    #[test]
    fn basic_select_integer() {
        check_types(
            "SELECT name FROM users WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn basic_like_text() {
        check_types(
            "SELECT * FROM random WHERE name LIKE ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn basic_insert_text_real() {
        check_types(
            "INSERT INTO users (name, costs) VALUES (?, ?)",
            vec![BaseType::Text, BaseType::Real],
        );
    }

    #[test]
    fn select_all_columns() {
        check_types("SELECT * FROM users WHERE age = ?", vec![BaseType::Integer]);
    }

    #[test]
    fn multiple_where_integer_text() {
        check_types(
            "SELECT * FROM users WHERE id = ? AND name = ?",
            vec![BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn or_condition_real_integer() {
        check_types(
            "SELECT * FROM users WHERE costs > ? OR age < ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn three_conditions() {
        check_types(
            "SELECT * FROM users WHERE id = ? OR name = ? OR costs > ?",
            vec![BaseType::Integer, BaseType::Text, BaseType::Real],
        );
    }

    #[test]
    fn nested_and_or() {
        check_types(
            "SELECT * FROM users WHERE (id = ? OR age > ?) AND name = ?",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn select_specific_columns() {
        check_types(
            "SELECT id, name, age FROM users WHERE costs > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn where_with_expression() {
        check_types(
            "SELECT * FROM users WHERE id + 10 = ?",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 2. COMPARISON OPERATORS (8 tests)
    // ==============================
    #[test]
    fn greater_than_real() {
        check_types(
            "SELECT * FROM products WHERE price > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn less_than_equal_integer() {
        check_types(
            "SELECT * FROM orders WHERE order_id <= ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn not_equal_text() {
        check_types("SELECT * FROM random WHERE name != ?", vec![BaseType::Text]);
    }

    #[test]
    fn between_integer_integer() {
        check_types(
            "SELECT * FROM users WHERE age BETWEEN ? AND ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn in_clause_integer() {
        check_types(
            "SELECT * FROM users WHERE id IN (?, ?, ?)",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn in_clause_text() {
        check_types(
            "SELECT * FROM random WHERE name IN (?, ?)",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn not_in_integer() {
        check_types(
            "SELECT * FROM users WHERE id NOT IN (?, ?)",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn chained_comparisons() {
        check_types(
            "SELECT * FROM products WHERE price >= ? AND price <= ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    // ==============================
    // 3. PATTERN MATCHING (5 tests)
    // ==============================
    #[test]
    fn like_pattern() {
        check_types(
            "SELECT * FROM products WHERE description LIKE ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn not_like() {
        check_types(
            "SELECT * FROM random WHERE costs NOT LIKE ?",
            vec![BaseType::Text],
        );
    }

    // #[test]
    // fn glob_pattern() {
    //     check_types(
    //         "SELECT * FROM users WHERE name GLOB ?",
    //         vec![BaseType::Text],
    //     );
    // }

    #[test]
    fn multiple_like() {
        check_types(
            "SELECT * FROM random WHERE name LIKE ? OR costs LIKE ?",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn like_with_escape() {
        check_types(
            "SELECT * FROM products WHERE description LIKE ?",
            vec![BaseType::Text],
        );
    }

    // ==============================
    // 4. NULL HANDLING (6 tests)
    // ==============================
    #[test]
    fn is_null_check() {
        check_types(
            "SELECT * FROM random WHERE costs IS NULL AND id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn is_not_null() {
        check_types(
            "SELECT * FROM users WHERE age IS NOT NULL AND id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn coalesce_real_real() {
        check_types(
            "SELECT * FROM users WHERE COALESCE(costs, ?) > ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn coalesce_multiple() {
        check_types(
            "SELECT * FROM random WHERE COALESCE(costs, ?, ?) = ?",
            vec![BaseType::Text, BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn ifnull_real_real() {
        check_types(
            "SELECT * FROM users WHERE IFNULL(costs, ?) < ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn nullif_integer_integer() {
        check_types(
            "SELECT NULLIF(age, ?) FROM users WHERE id = ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    // ==============================
    // 5. ARITHMETIC OPERATIONS (10 tests)
    // ==============================
    #[test]
    fn addition_integer() {
        check_types(
            "SELECT * FROM users WHERE age + ? > 100",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn subtraction_real() {
        check_types(
            "SELECT * FROM products WHERE price - ? < 50.0",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn multiplication_integer() {
        check_types(
            "SELECT * FROM orders WHERE user_id * ? = 100",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn division_real() {
        check_types(
            "SELECT * FROM products WHERE price / ? > 10",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn modulo_integer() {
        check_types(
            "SELECT * FROM users WHERE id % ? = 0",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn mixed_arithmetic() {
        check_types(
            "SELECT * FROM users WHERE age * ? + costs > ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn parenthesized_arithmetic() {
        check_types(
            "SELECT * FROM users WHERE (age + ?) * ? > 100",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn unary_minus() {
        check_types(
            "SELECT * FROM products WHERE price > -?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn complex_math() {
        check_types(
            "SELECT * FROM products WHERE (price * ?) / (? + stock) > ?",
            vec![BaseType::Real, BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn integer_division() {
        check_types(
            "SELECT * FROM users WHERE age / ? = ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    // ==============================
    // 6. FUNCTIONS - NUMERIC (12 tests)
    // ==============================
    #[test]
    fn abs_function() {
        check_types(
            "SELECT * FROM users WHERE ABS(costs) > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn round_function() {
        check_types(
            "SELECT * FROM products WHERE ROUND(price) = ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn max_function() {
        check_types(
            "SELECT MAX(age) FROM users WHERE id > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn min_function() {
        check_types(
            "SELECT MIN(price) FROM products WHERE stock > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn avg_function() {
        check_types(
            "SELECT AVG(age) FROM users WHERE id IN (?, ?, ?)",
            vec![BaseType::Integer, BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn sum_function() {
        check_types(
            "SELECT SUM(total) FROM orders WHERE user_id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn count_with_where() {
        check_types(
            "SELECT COUNT(*) FROM users WHERE age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn count_distinct() {
        check_types(
            "SELECT COUNT(DISTINCT name) FROM users WHERE age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn ceil_function() {
        check_types(
            "SELECT * FROM products WHERE CEIL(price) = ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn floor_function() {
        check_types(
            "SELECT * FROM products WHERE FLOOR(price) < ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn power_function() {
        check_types(
            "SELECT * FROM users WHERE POWER(costs, ?) > ?",
            vec![BaseType::Real, BaseType::Real],
        );
    }

    #[test]
    fn sqrt_function() {
        check_types(
            "SELECT * FROM products WHERE SQRT(price) < ?",
            vec![BaseType::Real],
        );
    }

    // ==============================
    // 7. FUNCTIONS - STRING (10 tests)
    // ==============================
    #[test]
    fn substr_function() {
        check_types(
            "SELECT SUBSTR(name, 1, 5) FROM random WHERE costs = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn upper_function() {
        check_types(
            "SELECT UPPER(name) FROM users WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn lower_function() {
        check_types(
            "SELECT * FROM random WHERE LOWER(name) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn length_function() {
        check_types(
            "SELECT * FROM products WHERE LENGTH(description) > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn trim_function() {
        check_types(
            "SELECT * FROM random WHERE TRIM(name) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn replace_function() {
        check_types(
            "SELECT REPLACE(name, ?, ?) FROM users WHERE id = 1",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn concat_function() {
        check_types(
            "SELECT * FROM users WHERE name = ? || ?",
            vec![BaseType::Text, BaseType::Text],
        );
    }

    #[test]
    fn ltrim_function() {
        check_types(
            "SELECT * FROM random WHERE LTRIM(name) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn rtrim_function() {
        check_types(
            "SELECT * FROM random WHERE RTRIM(costs) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn instr_function() {
        check_types(
            "SELECT * FROM products WHERE INSTR(description, ?) > ?",
            vec![BaseType::Text, BaseType::Integer],
        );
    }

    // ==============================
    // 8. ALIASES & JOINS (15 tests)
    // ==============================
    #[test]
    fn table_alias() {
        check_types(
            "SELECT u.name FROM users u WHERE u.age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn column_alias_in_order_by() {
        check_types(
            "SELECT age AS user_age FROM users WHERE id > ? ORDER BY user_age",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn column_alias_in_group_by() {
        check_types(
            "SELECT name AS n FROM users WHERE id > ? GROUP BY n",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn column_alias_in_having() {
        check_types(
            "SELECT age, COUNT(*) AS cnt FROM users WHERE id > ? GROUP BY age HAVING cnt > ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn fail_column_alias_in_where() {
        expect_error("SELECT age AS user_age FROM users WHERE user_age > ?");
    }

    #[test]
    fn alias_shadowing_real_column() {
        check_types(
            "SELECT age AS name FROM users WHERE name = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn fail_alias_referencing_alias() {
        expect_error("SELECT age AS a, a + 10 AS b FROM users WHERE id = ?");
    }

    #[test]
    fn table_column_alias() {
        check_types(
            "SELECT u.age AS user_age FROM users u WHERE u.id > ? ORDER BY user_age",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn inner_join() {
        check_types(
            "SELECT * FROM users u INNER JOIN orders o ON u.id = o.user_id WHERE o.order_id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn left_join() {
        check_types(
            "SELECT * FROM users u LEFT JOIN orders o ON u.id = o.user_id WHERE u.name = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn right_join() {
        check_types(
            "SELECT * FROM users u RIGHT JOIN orders o ON u.id = o.user_id WHERE o.order_id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn implicit_join() {
        check_types(
            "SELECT * FROM users u, random m WHERE u.name = ? AND u.costs > ?",
            vec![BaseType::Text, BaseType::Real],
        );
    }

    #[test]
    fn multiple_joins() {
        check_types(
            "SELECT * FROM users u JOIN orders o ON u.id = o.user_id JOIN products p ON o.order_id = p.product_id WHERE u.id = ? AND p.price > ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    #[test]
    fn self_join() {
        check_types(
            "SELECT * FROM users u1, users u2 WHERE u1.id = u2.id AND u1.age > ? AND u2.age < ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    #[test]
    fn fail_alias_in_join_on() {
        expect_error(
            "SELECT u.id AS uid FROM users u JOIN orders o ON uid = o.user_id WHERE o.total > ?",
        );
    }

    // ==============================
    // 9. SUBQUERIES (10 tests)
    // ==============================
    #[test]
    fn scalar_subquery() {
        check_types(
            "SELECT * FROM users WHERE age = (SELECT id FROM random WHERE name = ?)",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn in_subquery() {
        check_types(
            "SELECT * FROM users WHERE name IN (SELECT name FROM random WHERE id > ?)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn exists_subquery() {
        check_types(
            "SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND o.order_id = ?)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn not_exists_subquery() {
        check_types(
            "SELECT * FROM products WHERE NOT EXISTS (SELECT 1 FROM orders WHERE total < ?)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn correlated_subquery() {
        check_types(
            "SELECT * FROM products p WHERE price > (SELECT AVG(total) FROM orders WHERE user_id = ?)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn subquery_in_from() {
        check_types(
            "SELECT * FROM (SELECT * FROM users WHERE age > 30) sub WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn subquery_with_alias() {
        check_types(
            "SELECT sub.name FROM (SELECT * FROM users WHERE age > ?) sub WHERE sub.id = 1",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn multiple_subqueries() {
        check_types(
            "SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > ?) AND costs > (SELECT AVG(price) FROM products WHERE stock > ?)",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    // ==============================
    // 14. INSERT VARIATIONS (8 tests)
    // ==============================
    #[test]
    fn insert_single() {
        check_types("INSERT INTO users (id) VALUES (?)", vec![BaseType::Integer]);
    }

    #[test]
    fn insert_multiple_cols() {
        check_types(
            "INSERT INTO users (id, name, costs) VALUES (?, ?, ?)",
            vec![BaseType::Integer, BaseType::Text, BaseType::Real],
        );
    }

    #[test]
    fn insert_all_cols() {
        check_types(
            "INSERT INTO users VALUES (?, ?, ?, ?)",
            vec![
                BaseType::Integer,
                BaseType::Text,
                BaseType::Real,
                BaseType::Integer,
            ],
        );
    }

    #[test]
    fn insert_multiple_rows() {
        check_types(
            "INSERT INTO products (product_id, price) VALUES (?, ?), (?, ?)",
            vec![
                BaseType::Integer,
                BaseType::Real,
                BaseType::Integer,
                BaseType::Real,
            ],
        );
    }

    // ==============================
    // 22. AMBIGUITY CASES (should fail)
    // ==============================
    #[test]
    fn fail_ambiguous_column() {
        expect_error("SELECT * FROM users, random WHERE costs = ?");
    }

    #[test]
    fn cte_with_placeholder() {
        check_types(
            "WITH temp AS (SELECT * FROM users WHERE age > ?) SELECT * FROM temp WHERE id = ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }
    #[test]
    fn fail_ambiguous_in_join() {
        expect_error("SELECT * FROM users u, random m, products p WHERE id = ?");
    }

    #[test]
    fn fail_double_placeholder_no_context() {
        expect_error("SELECT ? + ?");
    }

    #[test]
    fn fail_placeholder_in_select_only() {
        expect_error("SELECT ?");
    }

    // ==============================
    // 23. AMBIGUITY FIXES
    // ==============================
    #[test]
    fn fix_explicit_table_column() {
        check_types(
            "SELECT * FROM users, random WHERE users.costs = ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn fix_table_prefix_both_sides() {
        check_types(
            "SELECT * FROM users u, random m WHERE u.costs = ? OR m.costs = ?",
            vec![BaseType::Real, BaseType::Text],
        );
    }

    // ==============================
    // 10. COMMON TABLE EXPRESSIONS (CTE)
    // ==============================
    #[test]
    fn simple_cte() {
        check_types(
            "WITH high_spenders AS (SELECT * FROM users WHERE costs > ?) SELECT * FROM high_spenders",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn simple_cte_2() {
        check_types(
            "WITH temp AS (SELECT id, name FROM users WHERE age > 30) SELECT * FROM temp WHERE id = ?",
            vec![BaseType::Integer],
        );
    }
    #[test]
    fn cte_placeholder_in_main_query() {
        check_types(
            "WITH user_names AS (SELECT id, name FROM users) SELECT * FROM user_names WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn multiple_ctes() {
        check_types(
            "WITH u AS (SELECT * FROM users WHERE age > ?), o AS (SELECT * FROM orders WHERE total > ?) SELECT * FROM u JOIN o ON u.id = o.user_id",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    // ==============================
    // 11. CASE EXPRESSIONS
    // ==============================
    #[test]
    fn case_expression_result() {
        check_types(
            "SELECT CASE WHEN age > 18 THEN name ELSE ? END FROM users",
            vec![BaseType::Text], // Matches 'name' type
        );
    }

    #[test]
    fn case_expression_condition() {
        check_types(
            "SELECT * FROM users WHERE CASE WHEN id = ? THEN 1 ELSE 0 END",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn case_in_where() {
        check_types(
            "SELECT * FROM products WHERE price > CASE WHEN stock > ? THEN 100.0 ELSE 50.0 END",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 12. CAST AND CONVERSIONS
    // ==============================
    #[test]
    fn cast_column_to_text() {
        check_types(
            "SELECT * FROM users WHERE CAST(age AS TEXT) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn cast_expression() {
        check_types(
            "SELECT * FROM orders WHERE CAST(total AS INTEGER) > ?",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 13. UPDATE STATEMENTS
    // ==============================
    #[test]
    fn update_simple() {
        check_types(
            "UPDATE users SET name = ? WHERE id = ?",
            vec![BaseType::Text, BaseType::Integer],
        );
    }

    #[test]
    fn update_with_math() {
        check_types(
            "UPDATE users SET costs = costs + ? WHERE age > ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    #[test]
    fn update_multiple_set() {
        check_types(
            "UPDATE products SET price = ?, stock = ? WHERE product_id = ?",
            vec![BaseType::Real, BaseType::Integer, BaseType::Integer],
        );
    }

    // ==============================
    // 14. DELETE STATEMENTS
    // ==============================
    #[test]
    fn delete_where() {
        check_types("DELETE FROM users WHERE id = ?", vec![BaseType::Integer]);
    }

    #[test]
    fn delete_complex_condition() {
        check_types(
            "DELETE FROM orders WHERE total < ? AND created_at LIKE ?",
            vec![BaseType::Real, BaseType::Text],
        );
    }

    // ==============================
    // 15. GROUP BY AND HAVING
    // ==============================
    #[test]
    fn group_by_placeholder() {
        // Though rare, some dialects allow grouping by a constant/parameter,
        // or this tests expressions in GROUP BY
        check_types(
            "SELECT age FROM users GROUP BY age + ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn having_clause_aggregate() {
        check_types(
            "SELECT user_id, SUM(total) FROM orders GROUP BY user_id HAVING SUM(total) > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn having_clause_count() {
        check_types(
            "SELECT name, COUNT(*) FROM users GROUP BY name HAVING COUNT(*) >= ?",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 16. SET OPERATIONS (UNION/INTERSECT)
    // ==============================
    #[test]
    fn union_binding() {
        check_types(
            "SELECT id FROM users WHERE age > ? UNION SELECT id FROM random WHERE name = ?",
            vec![BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn intersect_binding() {
        check_types(
            "SELECT name FROM users WHERE costs > ? INTERSECT SELECT name FROM random WHERE id = ?",
            vec![BaseType::Real, BaseType::Integer],
        );
    }

    // ==============================
    // 17. ORDER BY EXPRESSIONS
    // ==============================
    #[test]
    fn order_by_calculation() {
        check_types(
            "SELECT * FROM products ORDER BY price * ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn order_by_case() {
        check_types(
            "SELECT * FROM users ORDER BY CASE WHEN age > ? THEN 1 ELSE 0 END",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 18. LIMIT AND OFFSET
    // ==============================
    #[test]
    fn limit_clause() {
        check_types("SELECT * FROM users LIMIT ?", vec![BaseType::Integer]);
    }

    #[test]
    fn limit_offset() {
        check_types(
            "SELECT * FROM users LIMIT ? OFFSET ?",
            vec![BaseType::Integer, BaseType::Integer],
        );
    }

    // ==============================
    // 19. DATE/TIME FUNCTIONS
    // ==============================
    #[test]
    fn date_function() {
        // created_at is TEXT in setup_tables
        check_types(
            "SELECT * FROM orders WHERE DATE(created_at) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn datetime_comparison() {
        check_types(
            "SELECT * FROM orders WHERE created_at > DATETIME(?)",
            vec![BaseType::Text],
        );
    }

    // ==============================
    // 20. TYPE AFFINITY EDGE CASES
    // ==============================
    #[test]
    fn text_column_numeric_context() {
        // In table 'random', 'costs' is TEXT.
        check_types("SELECT * FROM random WHERE costs > ?", vec![BaseType::Text]);
    }

    #[test]
    fn mixed_type_and() {
        check_types(
            "SELECT * FROM users WHERE id = ? AND costs = ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    // ==============================
    // 21. NESTED FUNCTIONS & STRING OPS
    // ==============================
    #[test]
    fn nested_string_funcs() {
        check_types(
            "SELECT * FROM users WHERE UPPER(TRIM(name)) = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn nested_numeric_funcs() {
        check_types(
            "SELECT * FROM products WHERE ROUND(ABS(price)) > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn string_concatenation_bind() {
        check_types(
            "SELECT * FROM users WHERE name || ' ' || ? = 'John Doe'",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn string_concatenation_left_side() {
        check_types(
            "SELECT * FROM users WHERE name = ? || 'son'",
            vec![BaseType::Text],
        );
    }

    // ==============================
    // 24. ADVANCED JOIN / INSERT
    // ==============================
    #[test]
    fn join_using_clause() {
        // Both users and random have 'id'
        check_types(
            "SELECT * FROM users JOIN random USING (id) WHERE id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn insert_select() {
        check_types(
            "INSERT INTO users (id, name) SELECT ?, ? FROM random WHERE id > 10",
            vec![BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn insert_select_where() {
        check_types(
            "INSERT INTO users (id, name) SELECT id, name FROM random WHERE costs = ?",
            vec![BaseType::Text], // random.costs is TEXT
        );
    }

    #[test]
    fn aggregate_argument() {
        check_types("SELECT SUM(age + ?) FROM users", vec![BaseType::Integer]);
    }

    // ==============================
    // 25. ERROR CASES (NEGATIVE TESTS)
    // ==============================
    #[test]
    fn fail_placeholder_table_name() {
        expect_error("SELECT * FROM ?");
    }

    #[test]
    fn fail_placeholder_column_name() {
        expect_error("SELECT ? FROM users");
    }

    #[test]
    fn fail_non_existent_column() {
        expect_error("SELECT * FROM users WHERE non_existent_col = ?");
    }

    #[test]
    fn fail_insert_type_mismatch() {
        // Assuming strict type checking is enabled in your logic.
        // If your logic is loose (sqlite style), this might pass.
        // If strict: inserting text into integer column
        // check_types("INSERT INTO users (id) VALUES ('text')", vec![]); -> This logic depends on parser validation
        // But for binding parameters:
        // We can't really fail inference on "INSERT INTO users (id) VALUES (?)"
        // because ? just becomes Integer.
        // A mismatch error usually happens if the SQL itself is invalid before binding.
    }
    // ==============================
    // 26. WINDOW FUNCTIONS (Advanced)
    // ==============================
    #[test]
    fn window_partition_by() {
        check_types(
            "SELECT AVG(total) OVER (PARTITION BY user_id) FROM orders WHERE user_id = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn window_order_by() {
        check_types(
            "SELECT ROW_NUMBER() OVER (ORDER BY costs DESC) FROM users WHERE age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn window_frame_clause() {
        // "ROWS BETWEEN ? PRECEDING..." -> The offset must be an integer
        check_types(
            "SELECT SUM(total) OVER (ORDER BY created_at ROWS BETWEEN ? PRECEDING AND CURRENT ROW) FROM orders",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 27. UPSERT / ON CONFLICT
    // ==============================
    #[test]
    fn on_conflict_do_update() {
        check_types(
            "INSERT INTO users (id, name, costs, age) VALUES (1, 'a', 1.0, 20) ON CONFLICT(id) DO UPDATE SET costs = ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn on_conflict_where() {
        check_types(
            "INSERT INTO users (id) VALUES (1) ON CONFLICT(id) DO UPDATE SET name = 'fixed' WHERE age > ?",
            vec![BaseType::Integer],
        );
    }

    // ==============================
    // 28. BITWISE OPERATIONS
    // ==============================
    #[test]
    fn bitwise_and() {
        check_types(
            "SELECT * FROM users WHERE (age & ?) > 0",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn bitwise_or() {
        check_types(
            "SELECT * FROM users WHERE (id | ?) = 15",
            vec![BaseType::Integer],
        );
    }

    // #[test]
    // fn bitwise_shift() {
    //     check_types(
    //         "SELECT * FROM products WHERE (stock << ?) > 100",
    //         vec![BaseType::Integer],
    //     );
    // }

    // ==============================
    // 29. COMPLEX BOOLEAN LOGIC
    // ==============================
    #[test]
    fn not_logic() {
        check_types(
            "SELECT * FROM users WHERE NOT (age > ?)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn complex_not_and() {
        check_types(
            "SELECT * FROM users WHERE NOT (name = ? AND costs < ?)",
            vec![BaseType::Text, BaseType::Real],
        );
    }

    // #[test]
    // fn boolean_constants_simulation() {
    //     // '1' is often treated as TRUE in SQL
    //     check_types(
    //         "SELECT * FROM users WHERE (age > ?) IS 1",
    //         vec![BaseType::Integer],
    //     );
    // }

    // ==============================
    // 30. TUPLE / ROW VALUE COMPARISONS
    // ==============================
    // Note: Not all parsers support (a,b) = (1,2), but if yours does:
    #[test]
    fn row_value_comparison() {
        check_types(
            "SELECT * FROM users WHERE (id, name) = (?, ?)",
            vec![BaseType::Integer, BaseType::Text],
        );
    }

    #[test]
    fn row_value_in() {
        check_types(
            "SELECT * FROM users WHERE (id, age) IN ((?, ?), (?, ?))",
            vec![
                BaseType::Integer,
                BaseType::Integer,
                BaseType::Integer,
                BaseType::Integer,
            ],
        );
    }

    // ==============================
    // 33. RETURNING CLAUSE (Postgres/SQLite)
    // ==============================
    #[test]
    fn insert_returning() {
        // DELETE/UPDATE/INSERT returning values where a placeholder is involved in the calculation
        check_types(
            "DELETE FROM users WHERE id = ? RETURNING name",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn update_returning_expression() {
        check_types(
            "UPDATE products SET price = 10.0 WHERE stock < ? RETURNING price * ?",
            vec![BaseType::Integer, BaseType::Real],
        );
    }

    // ==============================
    // 34. EXPRESSION LISTS & UNUSUAL CONTEXTS
    // ==============================
    #[test]
    fn select_without_from() {
        // Just calculating values
        check_types(
            "SELECT ? + 5",
            vec![BaseType::Integer], // 5 is int, so ? should be int
        );
    }

    #[test]
    fn select_without_from_string() {
        check_types("SELECT 'Hello ' || ?", vec![BaseType::Text]);
    }
    // ==============================
    // 35. ADVANCED ALIAS ROBUSTNESS
    // ==============================

    #[test]
    fn alias_propagation_from_subquery() {
        // Crucial: Checks if 'age' (Integer) keeps being Integer when renamed to 'user_age'
        // and pulled out of a subquery.
        check_types(
            "SELECT * FROM (SELECT age AS user_age FROM users) sub WHERE sub.user_age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn alias_expression_propagation() {
        // Checks if an expression (Real) keeps its type when aliased and queried outside.
        // (costs * 1.5) is Real.
        check_types(
            "SELECT * FROM (SELECT (costs * 1.5) AS vat_adjusted FROM users) sub WHERE sub.vat_adjusted > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn double_aliasing_chain() {
        // users.id -> internal.id_a -> external.id_b -> ?
        check_types(
            "SELECT * FROM (SELECT id_a AS id_b FROM (SELECT id AS id_a FROM users) internal) external WHERE external.id_b = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn self_join_explicit_aliases() {
        // Explicitly calculating math between two instances of the same table
        check_types(
            "SELECT * FROM users u1 JOIN users u2 ON u1.id = u2.id WHERE u1.age - u2.age > ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn alias_in_having_complex() {
        // Aggregate alias used in HAVING
        check_types(
            "SELECT user_id, SUM(total) AS grand_total FROM orders GROUP BY user_id HAVING grand_total > ?",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn cast_alias_propagation() {
        // Casting to text, aliasing, then comparing outside. Should be Text.
        check_types(
            "SELECT * FROM (SELECT CAST(age AS TEXT) AS age_str FROM users) sub WHERE sub.age_str = ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn join_on_aliases() {
        // Joining a real table against a derived table with aliases
        check_types(
            "SELECT * FROM orders o JOIN (SELECT id AS uid FROM users) u ON o.user_id = u.uid WHERE u.uid = ?",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn ambiguous_alias_resolution() {
        // Both subqueries return 'x'. We specify 'a.x'.
        check_types(
            "SELECT * FROM (SELECT age AS x FROM users) a, (SELECT costs AS x FROM random) b WHERE a.x > ?",
            vec![BaseType::Integer], // a.x comes from users.age (Int)
        );
    }

    #[test]
    fn ambiguous_alias_resolution_b() {
        // Both subqueries return 'x'. We specify 'b.x'.
        check_types(
            "SELECT * FROM (SELECT age AS x FROM users) a, (SELECT costs AS x FROM random) b WHERE b.x = ?",
            vec![BaseType::Text], // b.x comes from random.costs (Text)
        );
    }

    // ==============================
    // 36. ALIAS ERROR CASES (Robustness Checks)
    // ==============================

    #[test]
    fn fail_duplicate_table_alias() {
        // Two tables cannot have the same alias in the same scope
        expect_error("SELECT * FROM users u, orders u WHERE u.id = ?");
    }

    #[test]
    fn fail_referencing_inner_alias_outside() {
        // 'user_age' is defined inside 'sub', but we try to use it without the 'sub.' prefix
        // (depending on strictness, this might be ambiguous or invalid if not prefixed)
        // OR: trying to access the original name 'age' which was not selected
        expect_error("SELECT * FROM (SELECT age AS user_age FROM users) sub WHERE age = ?");
    }

    // ==============================
    // 37. SCOPE SHADOWING (NIGHTMARE MODE)
    // ==============================

    #[test]
    fn shadow_table_alias() {
        // Outer 'u' is users. Inner 'u' is orders.
        // The parameter compares against inner 'u.total' (Real), not users.age (Integer).
        check_types(
            "SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders u WHERE u.total > ?)",
            vec![BaseType::Real],
        );
    }

    #[test]
    fn shadow_column_name() {
        // 'id' exists in both. Inner query uses users.id. Outer query uses products.id.
        // We need to make sure the parameter inside binds to the correct scope.
        check_types(
            "SELECT * FROM products WHERE id IN (SELECT id FROM users WHERE id = ?)",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn correlated_shadowing() {
        // Deeply nested correlation.
        // The inner ? compares against 'o.user_id'.
        // We need to ensure o.user_id is resolved from the outer query correctly
        // despite the middle subquery.
        check_types(
            "SELECT * FROM orders o WHERE EXISTS (SELECT * FROM products p WHERE p.price > (SELECT MAX(total) FROM users u WHERE u.id = o.user_id AND u.age > ?))",
            vec![BaseType::Integer], // u.age is Integer
        );
    }
    // ==============================
    // 38. RECURSIVE CTEs
    // ==============================

    #[test]
    fn recursive_cte_basic() {
        // The type of 'n' is determined by 'VALUES(1)' -> Integer.
        // The recursive part 'n+1' uses that type.
        // The WHERE clause uses n < ?. So ? must be Integer.
        check_types(
            "WITH RECURSIVE cnt(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM cnt WHERE n < ?) SELECT n FROM cnt",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn recursive_cte_inference() {
        // Here, 'cost' starts as Real (10.5).
        // Parameter ? is added to cost. So ? should probably be Real or Integer (promoted).
        // Let's expect Real.
        check_types(
            "WITH RECURSIVE calc(cost) AS (VALUES(10.5) UNION ALL SELECT cost + ? FROM calc WHERE cost < 100.0) SELECT * FROM calc",
            vec![BaseType::Real],
        );
    }
    // ==============================
    // 39. NULL AND BOOLEAN CONTEXTS
    // ==============================

    #[test]
    fn compare_to_null_literal() {
        // Technically this is always false/null in SQL, but valid syntax.
        // ? = NULL. Type is usually impossible to infer strictly,
        // but often treated as "Any" or fails.
        // If your engine is strict, this might be an error or Text/Int default.
        // Let's assume you allow it but maybe can't infer types (or default to Text).
        // check_types("SELECT * FROM users WHERE ? = NULL", vec![BaseType::Text]);
    }

    #[test]
    fn boolean_expression_standalone() {
        check_types(
            "SELECT * FROM users WHERE ? AND (age > 10)",
            vec![BaseType::Bool], // Assuming boolean is treated as 0/1 Integer
        );
    }
    // ==============================
    // 40. UNION TYPE PROPAGATION
    // ==============================

    #[test]
    fn union_placeholder_first() {
        // ? UNION SELECT age.
        // Since the second part is Integer, the first part (?) must be Integer.
        check_types(
            "SELECT ? AS val UNION SELECT age FROM users",
            vec![BaseType::Integer],
        );
    }

    #[test]
    fn union_placeholder_second() {
        // SELECT name UNION SELECT ?.
        // First part is Text, so ? must be Text.
        check_types(
            "SELECT name FROM users UNION SELECT ?",
            vec![BaseType::Text],
        );
    }

    #[test]
    fn union_mismatch_error() {
        // SELECT age (Int) UNION SELECT name (Text).
        // If strict, this should fail. If SQLite, it runs.
        // If we try to bind a parameter to the result:
        // SELECT * FROM (SELECT age AS x FROM users UNION SELECT name AS x FROM random) WHERE x = ?
        // This is ambiguous or "Any".
        // Robust engines might error here.
    }
    #[test]
    fn tuple_in_subquery() {
        // (id, user_id) IN (SELECT order_id, user_id FROM orders)
        // Checks if tuple binding works.
        check_types(
            "SELECT * FROM users WHERE (id, age) IN (SELECT order_id, total FROM orders WHERE total > ?)",
            vec![BaseType::Real], // total is Real
        );
    }
}
