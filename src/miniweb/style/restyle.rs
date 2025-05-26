use std::{collections::HashMap, hash::Hash};

use crate::miniweb::{
    dom::{self, DomElement, ElementOrText},
    layout::Vec2L,
    realm::symbol::Symbol,
    style::sheet::{RuleRankInfo, Selector, Stylesheet},
};

use super::{sheet::RuleStyle, ComputedStyle, DeclarationMap};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SingleSelectorRule {
    selector: Selector,
    rank: RuleRankInfo,
    style: RuleStyle,
}

// Allows narrowing down candidate simple selectors to a smaller group by
// taking the union of all the map lookups here.
struct SelectorMap<T> {
    name: HashMap<Symbol, Vec<T>>,
    id: HashMap<Symbol, Vec<T>>,
    class: HashMap<Symbol, Vec<T>>,
    other: Vec<T>,
}

impl<T> SelectorMap<T> {
    pub fn new() -> Self {
        Self {
            name: HashMap::new(),
            id: HashMap::new(),
            class: HashMap::new(),
            other: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.name.clear();
        self.id.clear();
        self.class.clear();
        self.other.clear();
    }
}

impl<T: Clone> SelectorMap<T> {
    pub fn insert(&mut self, selector: &Selector, entry: T) {
        if let Some(id) = &selector.id {
            self.id.entry(id.clone()).or_default().push(entry.clone());
        } else if let Some(class) = selector.classes.last() {
            self.class
                .entry(class.clone())
                .or_default()
                .push(entry.clone());
        } else if let Some(name) = &selector.name {
            self.name
                .entry(name.clone())
                .or_default()
                .push(entry.clone());
        } else {
            self.other.push(entry.clone());
        }
    }

    pub fn potential_matches(&self, object: &DomElement, out: &mut Vec<T>) {
        out.extend(self.other.iter().cloned());

        if let Some(values) = self.name.get(&object.name) {
            out.extend(values.iter().cloned());
        }

        if let Some(id) = &object.id {
            if let Some(values) = self.id.get(id) {
                out.extend(values.iter().cloned());
            }
        }

        for class in &object.classes {
            if let Some(values) = self.class.get(class) {
                out.extend(values.clone());
            }
        }
    }
}

pub struct StylesheetIndex {
    dirty: bool,
    stylesheets: Vec<Stylesheet>,
    selectors: SelectorMap<SingleSelectorRule>,
    cascade: Vec<SingleSelectorRule>,
}

impl StylesheetIndex {
    pub fn new() -> Self {
        Self {
            dirty: false,
            stylesheets: Vec::new(),
            selectors: SelectorMap::new(),
            cascade: Vec::new(),
        }
    }

    pub fn add_stylesheet(&mut self, sheet: Stylesheet) {
        let sheet_index = self.stylesheets.len();
        for (rule_index, rule) in sheet.rules.iter().enumerate() {
            let rank = RuleRankInfo::new(
                sheet.origin,
                false,
                rule.selectors
                    .iter()
                    .map(Selector::compute_specificity)
                    .max()
                    .unwrap_or_default(),
                sheet_index as u16,
                rule_index as u16,
            );

            for selector in &rule.selectors {
                let declarations = rule.declarations.clone();

                self.selectors.insert(
                    selector,
                    SingleSelectorRule {
                        selector: selector.clone(),
                        rank,
                        style: declarations.clone(),
                    },
                )
            }
        }

        self.stylesheets.push(sheet);
    }
}

pub struct StylingContext {
    pub time: u32,
    pub viewport_size: Vec2L,
}

impl StylesheetIndex {
    fn restyle_element(
        &mut self,
        ctx: &StylingContext,
        element: &mut dom::Element,
        parent: &ComputedStyle,
    ) {
        debug_assert!(self.cascade.is_empty());

        self.selectors
            .potential_matches(&element.object, &mut self.cascade);
        self.cascade
            .retain(|leaf| leaf.selector.matches(ctx, &element.object));
        self.cascade.sort_by_key(|x| x.rank);

        let mut specified = DeclarationMap::new();

        for selected in self.cascade.drain(..) {
            specified.merge(&selected.style.borrow());
        }

        let style = parent.create_child_with(ctx, &specified);

        element.object.style = style.clone();

        for child in &mut element.children {
            match child {
                ElementOrText::Element(element) => {
                    self.restyle_element(ctx, element, &style);
                }
                ElementOrText::Text(_) => (),
            }
        }
    }

    pub fn restyle(&mut self, ctx: &StylingContext, element: &mut dom::Element) {
        self.restyle_element(ctx, element, &ComputedStyle::default());
    }
}
