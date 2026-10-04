//! Persistent bounded numeric table. Rebuilt from the documented Native SDK contract.
use semwright_native_sdk::{Error, ErrorCode, Model, Operation, Result, Value, json, tokio};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub struct Table;
pub const MAX_CELLS: usize = 256;
pub const MAX_DEPTH: usize = 64;
pub const MAX_VISITS: usize = 16_384;
const MAX_VALUE: f64 = 1_000_000_000_000.0;

fn address(value: &str) -> bool {
    let b = value.as_bytes();
    (b.len() == 2 || b.len() == 3)
        && b[0].is_ascii_uppercase()
        && (b'1'..=b'9').contains(&b[1])
        && (b.len() == 2 || b[2].is_ascii_digit())
}

fn cells(state: &Value) -> Result<&serde_map::Map> {
    let outer = state
        .as_object()
        .ok_or_else(|| Error::invalid("Table state must be an object"))?;
    if outer.len() != 1 {
        return Err(Error::invalid("Table state requires only cells"));
    }
    let cells = outer
        .get("cells")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::invalid("Table cells must be an object"))?;
    if cells.is_empty() || cells.len() > MAX_CELLS {
        return Err(Error::invalid(
            "Table cell count exceeds the supported bounds",
        ));
    }
    for (name, value) in cells {
        if !address(name) {
            return Err(Error::invalid("Cell address must be A1 through Z99"));
        }
        if let Some(number) = value.as_f64() {
            if !number.is_finite() || number.abs() > MAX_VALUE {
                return Err(Error::invalid("Table numeric bound exceeded"));
            }
        } else {
            let formula = value
                .as_object()
                .filter(|v| v.len() == 1)
                .and_then(|v| v.get("sum"))
                .and_then(Value::as_array)
                .filter(|v| !v.is_empty() && v.len() <= 32)
                .ok_or_else(|| Error::invalid("Use a number or a bounded SUM formula"))?;
            if formula
                .iter()
                .any(|v| v.as_str().is_none_or(|v| !address(v)))
            {
                return Err(Error::invalid("SUM requires explicit valid cell addresses"));
            }
        }
    }
    Ok(cells)
}

// A local alias keeps generated consumers dependent only on the public SDK.
mod serde_map {
    pub type Map = semwright_native_sdk::serde_json::Map<String, super::Value>;
}

#[derive(Clone, Copy, Debug)]
struct Calculated {
    value: f64,
    dependency_depth: usize,
}
struct Evaluation<'a> {
    cells: &'a serde_map::Map,
    completed: BTreeMap<String, Calculated>,
    active: BTreeSet<String>,
    visits: usize,
    limit: usize,
}
impl<'a> Evaluation<'a> {
    fn new(cells: &'a serde_map::Map, limit: usize) -> Self {
        Self {
            cells,
            completed: BTreeMap::new(),
            active: BTreeSet::new(),
            visits: 0,
            limit,
        }
    }
    fn value(&mut self, cell: &str, depth: usize) -> Result<Calculated> {
        self.visits = self.visits.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Table work counter overflow")
        })?;
        if self.visits > self.limit || depth > MAX_DEPTH {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Table calculation budget exceeded",
            ));
        }
        if let Some(cached) = self.completed.get(cell) {
            if depth + cached.dependency_depth > MAX_DEPTH {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Table dependency depth exceeded",
                ));
            }
            return Ok(*cached);
        }
        if !self.active.insert(cell.into()) {
            return Err(Error::invalid("Table formula cycle"));
        }
        let cells = self.cells;
        let value = cells
            .get(cell)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "SUM input cell is missing"))?;
        let result = if let Some(number) = value.as_f64() {
            Calculated {
                value: number,
                dependency_depth: 0,
            }
        } else {
            let inputs = value["sum"]
                .as_array()
                .ok_or_else(|| Error::invalid("Invalid SUM formula"))?;
            let mut sum = 0.0;
            let mut longest = 0;
            for input in inputs {
                let row = self.value(
                    input
                        .as_str()
                        .ok_or_else(|| Error::invalid("Invalid SUM input"))?,
                    depth + 1,
                )?;
                sum += row.value;
                longest = longest.max(row.dependency_depth + 1);
                if !sum.is_finite() {
                    return Err(Error::invalid("Calculated table value is not finite"));
                }
            }
            if sum.abs() > MAX_VALUE {
                return Err(Error::invalid("Calculated table numeric bound exceeded"));
            }
            Calculated {
                value: sum,
                dependency_depth: longest,
            }
        };
        self.active.remove(cell);
        self.completed.insert(cell.into(), result);
        Ok(result)
    }
}

fn evaluate(state: &Value) -> Result<BTreeMap<String, f64>> {
    let cells = cells(state)?;
    let mut eval = Evaluation::new(cells, MAX_VISITS);
    let mut result = BTreeMap::new();
    for name in cells.keys() {
        result.insert(name.clone(), eval.value(name, 0)?.value);
    }
    Ok(result)
}

impl Model for Table {
    fn id(&self) -> &'static str {
        "native-table"
    }
    fn initial(&self) -> Value {
        json!({"cells":{"A1":10,"A2":20,"A3":{"sum":["A1","A2"]},"B1":99}})
    }
    fn validate(&self, state: &Value) -> Result<()> {
        evaluate(state).map(|_| ())
    }
    fn operations(&self) -> Vec<Operation> {
        vec![Operation {
            name: "set-cell",
            description: "Set a numeric cell or a bounded SUM formula",
            input_schema: json!({
                "type":"object", "$defs":{"cell":{"type":"string","pattern":"^[A-Z][1-9][0-9]?$"}},
                "properties":{"cell":{"$ref":"#/$defs/cell"},"value":{"oneOf":[
                    {"type":"number","minimum":-1000000000000.0,"maximum":1000000000000.0},
                    {"type":"object","properties":{"sum":{"type":"array","minItems":1,"maxItems":32,"items":{"$ref":"#/$defs/cell"}}},"required":["sum"],"additionalProperties":false}
                ]}},"required":["cell","value"],"additionalProperties":false
            }),
        }]
    }
    fn apply(&self, state: &Value, operation: &str, parameters: &Value) -> Result<Value> {
        if operation != "set-cell" {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Only bounded cell edits are supported",
            ));
        }
        self.validate(state)?;
        let p = parameters
            .as_object()
            .filter(|v| v.len() == 2 && v.contains_key("cell") && v.contains_key("value"))
            .ok_or_else(|| Error::invalid("Cell parameters require only cell and value"))?;
        let cell = p["cell"]
            .as_str()
            .filter(|v| address(v))
            .ok_or_else(|| Error::invalid("Invalid cell address"))?;
        let mut next = state.clone();
        next["cells"][cell] = p["value"].clone();
        self.validate(&next)?;
        Ok(next)
    }
    fn projection(&self, state: &Value) -> Result<Value> {
        Ok(
            json!({"rows":evaluate(state)?.into_iter().map(|(cell,value)|json!({"cell":cell,"value":value})).collect::<Vec<_>>()}),
        )
    }
    fn dependencies(&self, state: &Value) -> Result<Value> {
        self.validate(state)?;
        let cells = cells(state)?;
        let mut relations = BTreeSet::new();
        for (target, value) in cells {
            if let Some(inputs) = value.get("sum").and_then(Value::as_array) {
                for input in inputs {
                    relations.insert((input.as_str().unwrap().to_owned(), target.clone()));
                }
            }
        }
        Ok(
            json!({"relations":relations.into_iter().map(|(source,target)|json!({"source":source,"target":target,"kind":"formula_input"})).collect::<Vec<_>>(),"coverage":"complete_for_model"}),
        )
    }
    fn export(&self, state: &Value) -> Result<(String, Vec<u8>)> {
        let rows = evaluate(state)?;
        let mut csv = String::from("cell,value\n");
        for (cell, value) in rows {
            csv.push_str(&format!("{cell},{value}\n"));
        }
        Ok(("text/csv".into(), csv.into_bytes()))
    }
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    if let Err(error) = semwright_native_sdk::run(Table).await {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn shared_dependencies_are_memoized_without_changing_formula_order() {
        let state = json!({"cells":{"A1":2,"A2":{"sum":["A1","A1"]},"A3":{"sum":["A2","A2"]},"A4":{"sum":["A3","A3"]}}});
        let mut evaluation = Evaluation::new(cells(&state).unwrap(), 32);
        assert_eq!(evaluation.value("A4", 0).unwrap().value, 16.0);
        assert_eq!(evaluation.completed.len(), 4);
        assert_eq!(evaluation.visits, 7);
    }
    #[test]
    fn sum_keeps_declared_binary64_order_after_cache_hits() {
        let state = json!({"cells":{"A1":1e12,"A2":-1e12,"A3":1e-5,
            "B1":{"sum":["A1","A2","A3"]},"B2":{"sum":["A1","A3","A2"]}}});
        let rows = evaluate(&state).unwrap();
        assert_eq!(rows["B1"], 1e-5);
        assert_eq!(rows["B2"], 0.0);
    }
    #[test]
    fn cell_limit_allows_replacement_but_refuses_a_new_cell() {
        let mut map = serde_map::Map::new();
        for row in 1..=99 {
            for column in b'A'..=b'Z' {
                if map.len() == MAX_CELLS {
                    break;
                }
                map.insert(format!("{}{row}", column as char), json!(row));
            }
            if map.len() == MAX_CELLS {
                break;
            }
        }
        let state = json!({"cells":map});
        let replaced = Table
            .apply(&state, "set-cell", &json!({"cell":"A1","value":42}))
            .unwrap();
        assert_eq!(replaced["cells"].as_object().unwrap().len(), MAX_CELLS);
        assert_eq!(replaced["cells"]["A1"], 42);
        assert!(
            Table
                .apply(&state, "set-cell", &json!({"cell":"Z99","value":42}))
                .is_err()
        );
        assert_eq!(state["cells"]["A1"], 1);
        assert!(state["cells"].get("Z99").is_none());
    }
    #[test]
    fn small_work_budget_returns_an_error_before_a_partial_result() {
        let state = Table.initial();
        let mut evaluation = Evaluation::new(cells(&state).unwrap(), 1);
        assert_eq!(
            evaluation.value("A3", 0).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn cached_dependency_depth_cannot_hide_a_longer_path() {
        let mut map = serde_map::Map::new();
        map.insert("A1".into(), json!(1));
        for n in 2..=66 {
            map.insert(format!("A{n}"), json!({"sum":[format!("A{}",n-1)]}));
        }
        let state = json!({"cells":map});
        let mut evaluation = Evaluation::new(cells(&state).unwrap(), MAX_VISITS);
        assert_eq!(evaluation.value("A65", 0).unwrap().dependency_depth, 64);
        assert_eq!(
            evaluation.value("A66", 0).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn cycles_missing_inputs_and_numeric_overflow_remain_errors() {
        for state in [
            json!({"cells":{"A1":{"sum":["A2"]},"A2":{"sum":["A1"]}}}),
            json!({"cells":{"A1":{"sum":["A2"]}}}),
            json!({"cells":{"A1":1000000000000.0,"A2":{"sum":["A1","A1"]}}}),
        ] {
            assert!(Table.validate(&state).is_err());
        }
    }
}
