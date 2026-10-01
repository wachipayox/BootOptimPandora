//! Catalog ancestry uses verified selected revisions, with guards for missing parents/cycles.
use std::collections::{BTreeMap, BTreeSet};

pub type Parents = BTreeMap<String, Option<String>>;

pub fn root_id(id: &str, parents: &Parents) -> String {
    let mut current = id.to_owned();
    let mut path = Vec::new();
    loop {
        if let Some(start) = path.iter().position(|item| item == &current) {
            return path[start..].iter().min().cloned().unwrap_or(current);
        }
        path.push(current.clone());
        match parents.get(&current).and_then(Option::as_ref) {
            Some(parent) if parents.contains_key(parent) => current = parent.clone(),
            _ => return current,
        }
    }
}

pub fn is_ancestor(ancestor: &str, descendant: &str, parents: &Parents) -> bool {
    let mut current = descendant;
    let mut seen = BTreeSet::new();
    while seen.insert(current) {
        let Some(parent) = parents.get(current).and_then(Option::as_deref) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

pub fn related(left: &str, right: &str, parents: &Parents) -> bool {
    left != right && (is_ancestor(left, right, parents) || is_ancestor(right, left, parents))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ancestry_is_bidirectional_but_siblings_are_not_related() {
        let parents = Parents::from([
            ("root".into(), None),
            ("child".into(), Some("root".into())),
            ("grandchild".into(), Some("child".into())),
            ("sibling".into(), Some("root".into())),
        ]);
        assert!(related("root", "grandchild", &parents));
        assert!(related("grandchild", "root", &parents));
        assert!(!related("child", "sibling", &parents));
        assert_eq!(root_id("grandchild", &parents), "root");
    }
    #[test]
    fn orphans_and_cycles_remain_visible() {
        let parents = Parents::from([
            ("orphan".into(), Some("missing".into())),
            ("a".into(), Some("b".into())),
            ("b".into(), Some("a".into())),
        ]);
        assert_eq!(root_id("orphan", &parents), "orphan");
        assert_eq!(root_id("b", &parents), "a");
        assert!(!is_ancestor("missing", "a", &parents));
    }
}
