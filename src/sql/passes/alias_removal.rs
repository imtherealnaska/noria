use nom_sql::{Column, ConditionBase, ConditionExpression, ConditionTree, FieldExpression, SqlQuery};

use std::collections::HashMap;

pub trait AliasRemoval {
    fn expand_table_aliases(self) -> SqlQuery;
}

fn rewrite_conditional(table_aliases: &HashMap<String, String>,
                       ce: ConditionExpression)
                       -> ConditionExpression {
    let translate_column = |f: Column| {
        let new_f = match f.table {
            None => f,
            Some(t) => {
                Column {
                    name: f.name,
                    alias: f.alias,
                    table: if table_aliases.contains_key(&t) {
                        Some(table_aliases[&t].clone())
                    } else {
                        Some(t)
                    },
                    function: None,
                }
            }
        };
        ConditionExpression::Base(ConditionBase::Field(new_f))
    };

    let translate_ct_arm = |bce: Box<ConditionExpression>| -> Box<ConditionExpression> {
        let new_ce = match bce {
            box ConditionExpression::Base(ConditionBase::Field(f)) => translate_column(f),
            box ConditionExpression::Base(b) => ConditionExpression::Base(b),
            box x => rewrite_conditional(table_aliases, x),
        };
        Box::new(new_ce)
    };

    match ce {
        ConditionExpression::ComparisonOp(ct) => {
            let rewritten_ct = ConditionTree {
                operator: ct.operator,
                left: translate_ct_arm(ct.left),
                right: translate_ct_arm(ct.right),
            };
            ConditionExpression::ComparisonOp(rewritten_ct)
        }
        ConditionExpression::LogicalOp(ConditionTree { operator, box left, box right }) => {
            let rewritten_ct = ConditionTree {
                operator: operator,
                left: Box::new(rewrite_conditional(table_aliases, left)),
                right: Box::new(rewrite_conditional(table_aliases, right)),
            };
            ConditionExpression::LogicalOp(rewritten_ct)
        }
        x => x,
    }
}

impl AliasRemoval for SqlQuery {
    fn expand_table_aliases(self) -> SqlQuery {
        let mut table_aliases = HashMap::new();

        match self {
            SqlQuery::Select(mut sq) => {
                // Collect table aliases
                for t in &sq.tables {
                    match t.alias {
                        None => (),
                        Some(ref a) => {
                            table_aliases.insert(a.clone(), t.name.clone());
                        }
                    }
                }
                // Remove them from fields
                for field in sq.fields.iter_mut() {
                    match field {
                        &mut FieldExpression::Col(ref mut col) => {
                            if col.table.is_some() {
                                let t = col.table.take().unwrap();
                                col.table = if table_aliases.contains_key(&t) {
                                    Some(table_aliases[&t].clone())
                                } else {
                                    Some(t.clone())
                                };
                                col.function = None;
                            }
                        }
                        _ => {}
                    }
                }
                // Remove them from conditions
                sq.where_clause = match sq.where_clause {
                    None => None,
                    Some(wc) => Some(rewrite_conditional(&table_aliases, wc)),
                };
                SqlQuery::Select(sq)
            }
            // nothing to do for other query types, as they cannot have aliases
            x => x,
        }
    }
}

#[cfg(test)]
mod tests {
    use nom_sql::SelectStatement;
    use nom_sql::{Column, FieldExpression, SqlQuery, Table};
    use super::AliasRemoval;

    #[test]
    fn it_removes_aliases() {
        use nom_sql::{ConditionBase, ConditionExpression, ConditionTree, Operator};

        let wrap = |cb| Box::new(ConditionExpression::Base(cb));
        let q = SelectStatement {
            tables: vec![Table {
                             name: String::from("PaperTag"),
                             alias: Some(String::from("t")),
                         }],
            fields: vec![FieldExpression::Col(Column::from("t.id"))],
            where_clause: Some(ConditionExpression::ComparisonOp(ConditionTree {
                operator: Operator::Equal,
                left: wrap(ConditionBase::Field(Column::from("t.id"))),
                right: wrap(ConditionBase::Placeholder),
            })),
            ..Default::default()
        };
        let res = SqlQuery::Select(q).expand_table_aliases();
        // Table alias removed in field list
        match res {
            SqlQuery::Select(tq) => {
                assert_eq!(tq.fields,
                           vec![FieldExpression::Col(Column::from("PaperTag.id"))]);
                assert_eq!(tq.where_clause,
                           Some(ConditionExpression::ComparisonOp(ConditionTree {
                               operator: Operator::Equal,
                               left: wrap(ConditionBase::Field(Column::from("PaperTag.id"))),
                               right: wrap(ConditionBase::Placeholder),
                           })));
            }
            // if we get anything other than a selection query back, something really weird is up
            _ => panic!(),
        }
    }
}