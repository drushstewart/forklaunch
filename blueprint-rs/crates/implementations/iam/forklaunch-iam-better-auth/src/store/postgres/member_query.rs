//! Bounded, transient public projections evaluated by the configured database.
//! Identifiers/operators below are allowlisted; request data is always bound.
use super::{AuthTransaction, BlueprintError, BlueprintResult, Model, PostgresAuthTransaction};
use crate::store::AuthWindow;
use serde_json::{Value, json};

const MAX_RECORDS: usize = 4096;
const MAX_BYTES: usize = 1024 * 1024;

fn invalid() -> BlueprintError {
    BlueprintError::new(
        "HTTP_INTERNAL_EMPTY",
        "Unable to query organization members",
    )
}
fn budget() -> BlueprintError {
    BlueprintError::new(
        "better_auth_member_query_limit",
        "Organization member query exceeds its bounded projection budget",
    )
}
fn field(name: &str) -> BlueprintResult<(&'static str, &'static str)> {
    match name {
        "id" => Ok(("id", "uuid")),
        "organizationId" => Ok(("\"organizationId\"", "text")),
        "userId" => Ok(("\"userId\"", "text")),
        "role" => Ok(("role", "text")),
        "createdAt" => Ok(("\"createdAt\"", "timestamptz")),
        _ => Err(invalid()),
    }
}
// Check shape before serialization/cloning. A counting writer then bounds the
// exact JSON parameter size without allocating a second unbounded payload.
fn validate_query_budget(query: &Value) -> BlueprintResult<()> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= 64 * 1024)
                .ok_or_else(|| std::io::Error::other("query budget"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut pending = vec![(query, 0usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 4096 || depth > 16 {
            return Err(budget());
        }
        match value {
            Value::Array(values) => {
                if values.len() > 1024 {
                    return Err(budget());
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > 64 {
                    return Err(budget());
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
        if pending.len() > 4096 {
            return Err(budget());
        }
    }
    serde_json::to_writer(Counter(0), query).map_err(|_| budget())
}
struct Query {
    predicate: String,
    order: String,
    operand: Value,
    offset: String,
    limit: String,
}
impl Query {
    fn new(query: &Value) -> BlueprintResult<Self> {
        validate_query_budget(query)?;
        let mut operand = query.get("filterValue").cloned().unwrap_or(Value::Null);
        let predicate = if let Some(name) = query["filterField"].as_str().filter(|s| !s.is_empty())
        {
            let (column, kind) = field(name)?;
            let operator = query["filterOperator"].as_str().unwrap_or("eq");
            match operator {
                "eq" | "in" | "not_in" if operand.is_array() => {
                    let comparison = if operator == "not_in" { "NOT IN" } else { "IN" };
                    format!(
                        "{column} {comparison} (SELECT value::{kind} FROM jsonb_array_elements_text($3::jsonb))"
                    )
                }
                "eq" | "ne" if operand.is_null() => format!(
                    "{column} IS {}NULL",
                    if operator == "ne" { "NOT " } else { "" }
                ),
                "eq" | "ne" | "lt" | "lte" | "gt" | "gte" => {
                    if operand.is_array() || operand.is_object() {
                        return Err(invalid());
                    }
                    let operator = match operator {
                        "eq" => "=",
                        "ne" => "<>",
                        "lt" => "<",
                        "lte" => "<=",
                        "gt" => ">",
                        _ => ">=",
                    };
                    format!("{column} {operator} (($3::jsonb #>> '{{}}')::{kind})")
                }
                "contains" | "starts_with" | "ends_with" => {
                    let value =
                        crate::ecmascript::string(query.get("filterValue")).ok_or_else(invalid)?;
                    operand = Value::String(match operator {
                        "contains" => format!("%{value}%"),
                        "starts_with" => format!("{value}%"),
                        _ => format!("%{value}"),
                    });
                    format!("{column} LIKE ($3::jsonb #>> '{{}}')")
                }
                _ => return Err(invalid()),
            }
        } else {
            "TRUE".into()
        };
        let order = if let Some(name) = query["sortBy"].as_str().filter(|s| !s.is_empty()) {
            let (column, _) = field(name)?;
            let direction = match query["sortDirection"].as_str().unwrap_or("asc") {
                "asc" => "ASC",
                "desc" => "DESC",
                _ => return Err(invalid()),
            };
            format!("{column} {direction}, ordinal ASC")
        } else {
            "ordinal ASC".into()
        };
        let number = crate::organizations::query_number;
        let offset = number(query.get("offset"));
        let limit = number(query.get("limit"));
        Ok(Self {
            predicate,
            order,
            operand,
            offset: if offset.is_nan() {
                "0".into()
            } else {
                offset.to_string()
            },
            limit: if limit.is_nan() || limit == 0.0 {
                "100".into()
            } else {
                limit.to_string()
            },
        })
    }
}
impl PostgresAuthTransaction<'_> {
    pub(super) async fn query_member_window(
        &mut self,
        organization: &str,
        query: &Value,
    ) -> BlueprintResult<AuthWindow> {
        let query = Query::new(query)?;
        let (records, projection) = self.member_projection(organization).await?;
        // The JSON parameter contains public member fields, never credentials or
        // private user attributes. Database statement/parameter logging policy
        // remains an operator responsibility; the application logs neither.
        let sql = format!("WITH candidates AS MATERIALIZED (
            SELECT * FROM jsonb_to_recordset($1::jsonb) AS m(
                ordinal bigint, id uuid, \"organizationId\" text,
                \"userId\" text, role text, \"createdAt\" timestamptz)
            WHERE \"organizationId\"=$2
        ), filtered AS MATERIALIZED (SELECT * FROM candidates WHERE {} AND $3::jsonb IS NOT NULL),
        page AS (SELECT ordinal FROM filtered ORDER BY {} LIMIT $4::numeric::bigint OFFSET $5::numeric::bigint)
        SELECT (SELECT count(*) FROM filtered), ARRAY(SELECT ordinal FROM page)", query.predicate, query.order);
        let (total, ordinals): (i64, Vec<i64>) = sqlx::query_as(&sql)
            .bind(Value::Array(projection))
            .bind(organization)
            .bind(query.operand)
            .bind(query.limit)
            .bind(query.offset)
            .fetch_one(&mut *self.tx)
            .await
            .map_err(|_| invalid())?;
        let selected = ordinals
            .into_iter()
            .map(|ordinal| {
                usize::try_from(ordinal)
                    .ok()
                    .and_then(|i| records.get(i))
                    .cloned()
                    .ok_or_else(invalid)
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(AuthWindow {
            records: selected,
            total: usize::try_from(total).map_err(|_| invalid())?,
        })
    }
    async fn member_projection(
        &mut self,
        organization: &str,
    ) -> BlueprintResult<(Vec<Value>, Vec<Value>)> {
        let mut records = Vec::new();
        let mut projection = Vec::new();
        let mut bytes = 2usize;
        let mut after = None;
        loop {
            let page = self
                .find_page(Model::Member, "organizationId", organization, after, 256)
                .await?;
            for row in page.records {
                if records.len() == MAX_RECORDS {
                    return Err(budget());
                }
                let public = json!({"ordinal":records.len(), "id":row["id"], "organizationId":row["organizationId"], "userId":row["userId"], "role":row["role"], "createdAt":row["createdAt"]});
                bytes = bytes
                    .checked_add(
                        serde_json::to_vec(&public).map_err(|_| invalid())?.len()
                            + usize::from(!projection.is_empty()),
                    )
                    .ok_or_else(budget)?;
                if bytes > MAX_BYTES {
                    return Err(budget());
                }
                records.push(row);
                projection.push(public);
            }
            after = page.next;
            if after.is_none() {
                break;
            }
            if records.len() == MAX_RECORDS {
                return Err(budget());
            }
        }
        Ok((records, projection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_query_rejects_untrusted_identifiers_and_shapes() {
        for query in [
            json!({"filterField":"role) OR TRUE--"}),
            json!({"sortBy":"unknown"}),
            json!({"filterField":"role","filterOperator":"unknown"}),
            json!({"filterField":"role","filterOperator":"in","filterValue":"owner"}),
            json!({"filterField":"role","filterOperator":"ne","filterValue":["owner"]}),
            json!({"sortBy":"role","sortDirection":"desc; DROP TABLE records"}),
        ] {
            assert!(Query::new(&query).is_err());
        }
    }

    #[test]
    fn member_query_rejects_oversized_operands_before_cloning() {
        for value in [json!("x".repeat(65536)), json!(vec!["x"; 1025])] {
            let error = Query::new(&json!({"filterField":"role","filterValue":value}))
                .err()
                .unwrap();
            assert_eq!(error.code, "better_auth_member_query_limit");
        }
    }

    #[test]
    fn member_query_binds_pattern_and_preserves_postgres_numeric_cast() {
        let query = Query::new(&json!({"filterField":"role","filterOperator":"contains","filterValue":"x%' OR TRUE--","sortBy":"createdAt","sortDirection":"desc","offset":"0.5","limit":"1.5"})).unwrap();
        assert_eq!(query.predicate, "role LIKE ($3::jsonb #>> '{}')");
        assert_eq!(query.operand, "%x%' OR TRUE--%");
        assert_eq!(query.offset, "0.5");
        assert_eq!(query.limit, "1.5");
        assert_eq!(query.order, "\"createdAt\" DESC, ordinal ASC");
        let defaults = Query::new(&json!({"limit":0,"offset":"bad"})).unwrap();
        assert_eq!(
            (defaults.limit.as_str(), defaults.offset.as_str()),
            ("100", "0")
        );
    }
}
