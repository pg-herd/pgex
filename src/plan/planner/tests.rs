use super::plan_source_tables;
use crate::{
    catalog::{Catalog, ColumnLayout, PgType, TableLayout},
    config::compiled::{ColumnRule, SourceFilterPolicy, TableFilter},
    predicate::Predicate,
};

#[test]
fn chooses_table_filter_then_first_matching_rule_then_shared_filter() {
    let catalog = Catalog {
        tables: vec![
            layout("explicit", &["a", "b"]),
            layout("two_matches", &["a", "b"]),
            layout("one_match", &["b"]),
            layout("fallback", &["c"]),
        ],
    };
    let filters = SourceFilterPolicy {
        shared: Some(predicate("shared")),
        table_rules: vec![
            ColumnRule {
                has_column: "a".to_owned(),
                condition: predicate("rule_a"),
            },
            ColumnRule {
                has_column: "b".to_owned(),
                condition: predicate("rule_b"),
            },
        ],
        table_filters: vec![TableFilter {
            table: "public.explicit".parse().unwrap(),
            condition: predicate("explicit"),
        }],
    };

    let source_plan = plan_source_tables(&catalog, &filters).unwrap();
    let clauses: Vec<_> = source_plan
        .tables
        .iter()
        .map(|table| table.filter.unwrap().sql.as_str())
        .collect();
    assert_eq!(clauses, ["explicit", "rule_a", "rule_b", "shared"]);
}

#[test]
fn chooses_all_rows_when_no_filter_matches() {
    let catalog = Catalog {
        tables: vec![layout("items", &["id"])],
    };
    let filters = SourceFilterPolicy {
        shared: None,
        table_rules: vec![ColumnRule {
            has_column: "missing".into(),
            condition: predicate("unused"),
        }],
        table_filters: Vec::new(),
    };

    let source_plan = plan_source_tables(&catalog, &filters).unwrap();
    assert!(source_plan.tables[0].filter.is_none());
}

fn layout(name: &str, columns: &[&str]) -> TableLayout {
    TableLayout {
        name: format!("public.{name}").parse().unwrap(),
        relation_oid: 0,
        columns: columns
            .iter()
            .map(|&name| ColumnLayout {
                name: name.into(),
                data_type: PgType {
                    schema: "pg_catalog".into(),
                    name: "int4".into(),
                    modifier: -1,
                    enum_labels: None,
                },
                nullable: true,
                generated: false,
            })
            .collect(),
    }
}

fn predicate(sql: &str) -> Predicate {
    Predicate {
        sql: sql.into(),
        parameters: Vec::new(),
    }
}
