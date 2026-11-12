//! Exact packing-list optimization with required items, dependencies, and choice groups.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const MAX_ITEMS: usize = 24;

/// A complete packing problem.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Capacity constraints for the bag.
    pub bag: Bag,
    /// Candidate items.
    #[serde(rename = "item")]
    pub items: Vec<Item>,
}

/// Bag capacity in integer grams and millilitres.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bag {
    pub name: String,
    pub max_weight_grams: u32,
    pub max_volume_ml: u32,
}

/// One packable item.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub weight_grams: u32,
    pub volume_ml: u32,
    pub score: u32,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
}

/// A deterministic optimal plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub bag: String,
    pub selected: Vec<PlannedItem>,
    pub total_score: u64,
    pub total_weight_grams: u64,
    pub total_volume_ml: u64,
    pub remaining_weight_grams: u64,
    pub remaining_volume_ml: u64,
}

/// An item selected by the optimizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedItem {
    pub id: String,
    pub name: String,
    pub weight_grams: u32,
    pub volume_ml: u32,
    pub score: u32,
    pub required: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid manifest: {0}")]
    Invalid(String),
    #[error("no feasible packing plan: {0}")]
    Infeasible(String),
}

/// Load and strictly parse a TOML manifest.
///
/// # Errors
///
/// Returns an error when the file cannot be read or is not valid strict TOML.
pub fn load_manifest(path: &Path) -> Result<Manifest, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Find the exact optimal feasible packing plan.
///
/// # Errors
///
/// Returns an error for an invalid manifest or when its hard constraints are infeasible.
pub fn optimize(manifest: &Manifest) -> Result<Plan, Error> {
    validate(manifest)?;
    let order = topological_order(&manifest.items)?;
    let forced = required_closure(&manifest.items)?;
    let mut search = Search::new(manifest, order, forced);
    search.run(0, Totals::default());
    let selected = search.best.ok_or_else(|| {
        Error::Infeasible(
            "required items, dependencies, groups, and bag capacities cannot all be satisfied"
                .to_owned(),
        )
    })?;
    Ok(build_plan(manifest, &selected))
}

/// Render a compact terminal-friendly plan.
#[must_use]
pub fn render_text(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", plan.bag);
    let _ = writeln!(out, "{}", "=".repeat(plan.bag.chars().count()));
    for item in &plan.selected {
        let marker = if item.required { "required" } else { "chosen" };
        let _ = writeln!(
            out,
            "[x] {} ({}, {} g, {} ml, score {})",
            item.name, marker, item.weight_grams, item.volume_ml, item.score
        );
    }
    let _ = writeln!(
        out,
        "\n{} items | score {} | {} g | {} ml",
        plan.selected.len(),
        plan.total_score,
        plan.total_weight_grams,
        plan.total_volume_ml
    );
    let _ = writeln!(
        out,
        "remaining: {} g and {} ml",
        plan.remaining_weight_grams, plan.remaining_volume_ml
    );
    out
}

/// Render a reusable Markdown checklist.
#[must_use]
pub fn render_markdown(plan: &Plan) -> String {
    let mut out = format!("# {} packing list\n\n", plan.bag);
    for item in &plan.selected {
        let note = if item.required {
            "required"
        } else {
            "selected"
        };
        let _ = writeln!(
            out,
            "- [ ] **{}** — {} g, {} ml, score {} ({})",
            item.name, item.weight_grams, item.volume_ml, item.score, note
        );
    }
    let _ = writeln!(
        out,
        "\n## Totals\n\n- Score: {}\n- Weight: {} g ({} g remaining)\n- Volume: {} ml ({} ml remaining)",
        plan.total_score,
        plan.total_weight_grams,
        plan.remaining_weight_grams,
        plan.total_volume_ml,
        plan.remaining_volume_ml
    );
    out
}

fn validate(manifest: &Manifest) -> Result<(), Error> {
    if manifest.bag.name.trim().is_empty() {
        return Err(Error::Invalid("bag name must not be empty".to_owned()));
    }
    if manifest.bag.max_weight_grams == 0 || manifest.bag.max_volume_ml == 0 {
        return Err(Error::Invalid(
            "bag capacities must both be greater than zero".to_owned(),
        ));
    }
    if manifest.items.is_empty() {
        return Err(Error::Invalid("at least one item is required".to_owned()));
    }
    if manifest.items.len() > MAX_ITEMS {
        return Err(Error::Invalid(format!(
            "{} items exceeds the exact-search limit of {MAX_ITEMS}",
            manifest.items.len()
        )));
    }

    let mut ids = BTreeSet::new();
    for item in &manifest.items {
        if item.id.trim().is_empty() || item.name.trim().is_empty() {
            return Err(Error::Invalid(
                "item ids and names must not be empty".to_owned(),
            ));
        }
        if !ids.insert(item.id.as_str()) {
            return Err(Error::Invalid(format!("duplicate item id: {}", item.id)));
        }
        if item.score == 0 {
            return Err(Error::Invalid(format!(
                "item {} must have a positive score",
                item.id
            )));
        }
        if item
            .group
            .as_ref()
            .is_some_and(|group| group.trim().is_empty())
        {
            return Err(Error::Invalid(format!(
                "item {} has an empty group",
                item.id
            )));
        }
    }
    for item in &manifest.items {
        let mut needs = BTreeSet::new();
        for dependency in &item.needs {
            if dependency == &item.id {
                return Err(Error::Invalid(format!(
                    "item {} cannot depend on itself",
                    item.id
                )));
            }
            if !ids.contains(dependency.as_str()) {
                return Err(Error::Invalid(format!(
                    "item {} needs unknown item {dependency}",
                    item.id
                )));
            }
            if !needs.insert(dependency) {
                return Err(Error::Invalid(format!(
                    "item {} repeats dependency {dependency}",
                    item.id
                )));
            }
        }
    }
    Ok(())
}

fn topological_order(items: &[Item]) -> Result<Vec<usize>, Error> {
    let by_id: BTreeMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.as_str(), index))
        .collect();
    let mut indegree = vec![0_usize; items.len()];
    let mut dependents = vec![Vec::new(); items.len()];
    for (index, item) in items.iter().enumerate() {
        indegree[index] = item.needs.len();
        for dependency in &item.needs {
            dependents[by_id[dependency.as_str()]].push(index);
        }
    }
    let mut ready: BTreeSet<(&str, usize)> = items
        .iter()
        .enumerate()
        .filter(|(index, _)| indegree[*index] == 0)
        .map(|(index, item)| (item.id.as_str(), index))
        .collect();
    let mut order = Vec::with_capacity(items.len());
    while let Some(&(id, index)) = ready.first() {
        ready.remove(&(id, index));
        order.push(index);
        for &dependent in &dependents[index] {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.insert((items[dependent].id.as_str(), dependent));
            }
        }
    }
    if order.len() != items.len() {
        return Err(Error::Invalid(
            "item dependencies must not contain a cycle".to_owned(),
        ));
    }
    Ok(order)
}

fn required_closure(items: &[Item]) -> Result<Vec<bool>, Error> {
    let by_id: BTreeMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.as_str(), index))
        .collect();
    let mut forced: Vec<bool> = items.iter().map(|item| item.required).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (index, item) in items.iter().enumerate() {
            if forced[index] {
                for dependency in &item.needs {
                    let dependency_index = *by_id.get(dependency.as_str()).ok_or_else(|| {
                        Error::Invalid(format!("item {} needs unknown item {dependency}", item.id))
                    })?;
                    if !forced[dependency_index] {
                        forced[dependency_index] = true;
                        changed = true;
                    }
                }
            }
        }
    }
    Ok(forced)
}

#[derive(Debug, Clone, Copy, Default)]
struct Totals {
    score: u64,
    weight: u64,
    volume: u64,
}

struct Search<'a> {
    manifest: &'a Manifest,
    order: Vec<usize>,
    forced: Vec<bool>,
    selected: Vec<bool>,
    remaining_score: Vec<u64>,
    best: Option<Vec<bool>>,
}

impl<'a> Search<'a> {
    fn new(manifest: &'a Manifest, order: Vec<usize>, forced: Vec<bool>) -> Self {
        let mut remaining_score = vec![0_u64; order.len() + 1];
        for position in (0..order.len()).rev() {
            remaining_score[position] =
                remaining_score[position + 1] + u64::from(manifest.items[order[position]].score);
        }
        Self {
            manifest,
            order,
            forced,
            selected: vec![false; manifest.items.len()],
            remaining_score,
            best: None,
        }
    }

    fn run(&mut self, position: usize, totals: Totals) {
        if totals.weight > u64::from(self.manifest.bag.max_weight_grams)
            || totals.volume > u64::from(self.manifest.bag.max_volume_ml)
        {
            return;
        }
        if let Some(best) = &self.best {
            let best_score = selected_totals(&self.manifest.items, best).score;
            if totals.score + self.remaining_score[position] < best_score {
                return;
            }
        }
        if position == self.order.len() {
            if self.is_better(totals) {
                self.best = Some(self.selected.clone());
            }
            return;
        }

        let index = self.order[position];
        if !self.forced[index] {
            self.run(position + 1, totals);
        }

        if self.can_include(index) {
            self.selected[index] = true;
            let item = &self.manifest.items[index];
            self.run(
                position + 1,
                Totals {
                    score: totals.score + u64::from(item.score),
                    weight: totals.weight + u64::from(item.weight_grams),
                    volume: totals.volume + u64::from(item.volume_ml),
                },
            );
            self.selected[index] = false;
        }
    }

    fn can_include(&self, index: usize) -> bool {
        let item = &self.manifest.items[index];
        if item.needs.iter().any(|needed| {
            !self
                .manifest
                .items
                .iter()
                .position(|candidate| candidate.id == *needed)
                .is_some_and(|needed_index| self.selected[needed_index])
        }) {
            return false;
        }
        item.group.as_ref().is_none_or(|group| {
            !self.selected.iter().enumerate().any(|(other, selected)| {
                *selected && self.manifest.items[other].group.as_ref() == Some(group)
            })
        })
    }

    fn is_better(&self, totals: Totals) -> bool {
        let Some(best) = &self.best else {
            return true;
        };
        let best_totals = selected_totals(&self.manifest.items, best);
        match totals.score.cmp(&best_totals.score) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match totals.weight.cmp(&best_totals.weight) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => match totals.volume.cmp(&best_totals.volume) {
                    Ordering::Less => true,
                    Ordering::Greater => false,
                    Ordering::Equal => {
                        selected_ids(&self.manifest.items, &self.selected)
                            < selected_ids(&self.manifest.items, best)
                    }
                },
            },
        }
    }
}

fn selected_totals(items: &[Item], selected: &[bool]) -> Totals {
    items
        .iter()
        .zip(selected)
        .filter(|(_, selected)| **selected)
        .fold(Totals::default(), |totals, (item, _)| Totals {
            score: totals.score + u64::from(item.score),
            weight: totals.weight + u64::from(item.weight_grams),
            volume: totals.volume + u64::from(item.volume_ml),
        })
}

fn selected_ids<'a>(items: &'a [Item], selected: &[bool]) -> Vec<&'a str> {
    let mut ids: Vec<_> = items
        .iter()
        .zip(selected)
        .filter(|(_, selected)| **selected)
        .map(|(item, _)| item.id.as_str())
        .collect();
    ids.sort_unstable();
    ids
}

fn build_plan(manifest: &Manifest, selected: &[bool]) -> Plan {
    let totals = selected_totals(&manifest.items, selected);
    let mut planned: Vec<_> = manifest
        .items
        .iter()
        .zip(selected)
        .filter(|(_, selected)| **selected)
        .map(|(item, _)| PlannedItem {
            id: item.id.clone(),
            name: item.name.clone(),
            weight_grams: item.weight_grams,
            volume_ml: item.volume_ml,
            score: item.score,
            required: item.required,
        })
        .collect();
    planned.sort_by(|left, right| left.id.cmp(&right.id));
    Plan {
        bag: manifest.bag.name.clone(),
        selected: planned,
        total_score: totals.score,
        total_weight_grams: totals.weight,
        total_volume_ml: totals.volume,
        remaining_weight_grams: u64::from(manifest.bag.max_weight_grams) - totals.weight,
        remaining_volume_ml: u64::from(manifest.bag.max_volume_ml) - totals.volume,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Manifest {
        toml::from_str(input).expect("valid test manifest")
    }

    #[test]
    fn chooses_best_feasible_combination() {
        let manifest = parse(
            r#"
                [bag]
                name = "Small bag"
                max_weight_grams = 1000
                max_volume_ml = 1000

                [[item]]
                id = "coat"
                name = "Coat"
                weight_grams = 600
                volume_ml = 600
                score = 7

                [[item]]
                id = "shirt"
                name = "Shirt"
                weight_grams = 300
                volume_ml = 300
                score = 5

                [[item]]
                id = "socks"
                name = "Socks"
                weight_grams = 200
                volume_ml = 200
                score = 4
            "#,
        );
        let plan = optimize(&manifest).expect("feasible");
        let ids: Vec<_> = plan.selected.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["coat", "shirt"]);
        assert_eq!(plan.total_score, 12);
    }

    #[test]
    fn pulls_in_dependencies_for_required_items() {
        let manifest = parse(
            r#"
                [bag]
                name = "Camera bag"
                max_weight_grams = 1000
                max_volume_ml = 1000

                [[item]]
                id = "battery"
                name = "Battery"
                weight_grams = 100
                volume_ml = 50
                score = 1

                [[item]]
                id = "camera"
                name = "Camera"
                weight_grams = 500
                volume_ml = 400
                score = 10
                required = true
                needs = ["battery"]
            "#,
        );
        let plan = optimize(&manifest).expect("feasible");
        assert_eq!(plan.selected.len(), 2);
        assert!(plan.selected.iter().any(|item| item.id == "battery"));
    }

    #[test]
    fn honors_mutually_exclusive_groups() {
        let manifest = parse(
            r#"
                [bag]
                name = "Day bag"
                max_weight_grams = 2000
                max_volume_ml = 2000

                [[item]]
                id = "boots"
                name = "Boots"
                weight_grams = 900
                volume_ml = 900
                score = 9
                group = "footwear"

                [[item]]
                id = "shoes"
                name = "Shoes"
                weight_grams = 500
                volume_ml = 500
                score = 8
                group = "footwear"
            "#,
        );
        let plan = optimize(&manifest).expect("feasible");
        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].id, "boots");
    }

    #[test]
    fn reports_infeasible_required_load() {
        let manifest = parse(
            r#"
                [bag]
                name = "Tiny bag"
                max_weight_grams = 100
                max_volume_ml = 100

                [[item]]
                id = "brick"
                name = "Brick"
                weight_grams = 1000
                volume_ml = 500
                score = 1
                required = true
            "#,
        );
        assert!(matches!(optimize(&manifest), Err(Error::Infeasible(_))));
    }

    #[test]
    fn rejects_dependency_cycles() {
        let manifest = parse(
            r#"
                [bag]
                name = "Bag"
                max_weight_grams = 1000
                max_volume_ml = 1000

                [[item]]
                id = "a"
                name = "A"
                weight_grams = 1
                volume_ml = 1
                score = 1
                needs = ["b"]

                [[item]]
                id = "b"
                name = "B"
                weight_grams = 1
                volume_ml = 1
                score = 1
                needs = ["a"]
            "#,
        );
        assert!(matches!(optimize(&manifest), Err(Error::Invalid(_))));
    }

    #[test]
    fn tie_breaks_by_weight_then_volume_then_ids() {
        let manifest = parse(
            r#"
                [bag]
                name = "Bag"
                max_weight_grams = 1000
                max_volume_ml = 1000

                [[item]]
                id = "b"
                name = "B"
                weight_grams = 100
                volume_ml = 100
                score = 5
                group = "one"

                [[item]]
                id = "a"
                name = "A"
                weight_grams = 100
                volume_ml = 100
                score = 5
                group = "one"
            "#,
        );
        let plan = optimize(&manifest).expect("feasible");
        assert_eq!(plan.selected[0].id, "a");
    }
}
