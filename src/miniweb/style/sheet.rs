use std::{
    cell::RefCell,
    hash::Hash,
    ops::{Deref, Range},
    rc::Rc,
};

use util::BoolExt;

use super::{restyle::StylingContext, DeclarationMap};
use crate::miniweb::{dom::DomElement, realm::symbol::Symbol};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
// Make this part of a reference counted structure or something
pub struct Selector {
    pub name: Option<Symbol>,
    pub id: Option<Symbol>,
    pub classes: Vec<Symbol>,
    pub future: bool,
    pub past: bool,
    // :-sbr-time() extension, only matches when the current time is inside this range
    pub time_range: Range<u32>,
}

impl Default for Selector {
    fn default() -> Self {
        Self {
            name: None,
            id: None,
            classes: Vec::new(),
            future: false,
            past: false,
            time_range: 0..u32::MAX,
        }
    }
}

macro_rules! selector {
    (@main $realm: ident $result: ident; # $id: literal $($rest: tt)*) => {
        $result.id = Some($realm.symbol($id));
        selector!(@main $realm $result; $($rest)*);
    };
    (@main $realm: ident $result: ident; . $class: literal $($rest: tt)*) => {
        $result.classes.push($realm.symbol($class));
        selector!(@main $realm $result; $($rest)*);
    };
    (@main $realm: ident $result: ident; $name: literal $($rest: tt)*) => {
        $result.name = Some($realm.symbol($name));
        selector!(@main $realm $result; $($rest)*);
    };
    (@main $realm: ident $result: ident; :future $($rest: tt)*) => {
        $result.future = true;
        selector!(@main $realm $result; $($rest)*);
    };
    (@main $realm: ident $result: ident; :past $($rest: tt)*) => {
        $result.past = true;
        selector!(@main $realm $result; $($rest)*);
    };
    (@main $realm: ident $result: ident;) => {};
    (in $realm: expr; $($args: tt)*) => {{
        let _realm = &$realm;
        let mut result = $crate::miniweb::style::sheet::Selector::default();
        selector!(@main _realm result; $($args)*);
        result
    }};
}
pub(crate) use selector;

impl Selector {
    pub fn matches(&self, ctx: &StylingContext, object: &DomElement) -> bool {
        self.name.as_ref().is_none_or(|name| name == &object.name)
            && self
                .id
                .as_ref()
                .is_none_or(|id| Some(id) == object.id.as_ref())
            && self
                .classes
                .iter()
                .all(|class| object.classes.contains(class))
            // TODO: check in spec whether these operators are correct
            && self.past.implies(object.time <= ctx.time)
            && self.future.implies(object.time > ctx.time)
            && self.time_range.contains(&ctx.time)
    }

    pub fn compute_specificity(&self) -> Specificity {
        Specificity(
            self.id.is_some() as u8,
            u8::try_from(self.classes.len())
                .unwrap_or(u8::MAX)
                .saturating_add(
                    self.future as u8 + self.past as u8 + (self.time_range != (0..u32::MAX)) as u8,
                ),
            self.name.is_some() as u8,
        )
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Author = 2,
    User = 1,
    UserAgent = 0,
}

#[derive(Default, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity(u8, u8, u8);

impl Specificity {
    fn max(self, rhs: Self) -> Self {
        std::cmp::max(self, rhs)
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuleRankInfo {
    origin_and_importance: u8,
    specificity: Specificity,
    stylesheet_index: u16,
    rule_index: u16,
}

impl RuleRankInfo {
    const MIN: Self = Self {
        origin_and_importance: 0,
        specificity: Specificity(0, 0, 0),
        stylesheet_index: 0,
        rule_index: 0,
    };

    pub fn new(
        origin: Origin,
        important: bool,
        specificity: Specificity,
        stylesheet_index: u16,
        rule_index: u16,
    ) -> Self {
        Self {
            origin_and_importance: Self::combine_origin_and_importance(origin, important),
            specificity,
            stylesheet_index,
            rule_index,
        }
    }

    fn combine_origin_and_importance(origin: Origin, important: bool) -> u8 {
        let mask = (((important as i8) << 7) >> 7) as u8;
        (origin as u8) ^ mask
    }

    pub fn origin(&self) -> Origin {
        let inv_mask = ((self.origin_and_importance as i8) >> 7) as u8;
        match (self.origin_and_importance ^ inv_mask) & 0b11 {
            2 => Origin::Author,
            1 => Origin::User,
            0 => Origin::UserAgent,
            _ => unreachable!(),
        }
    }

    pub fn important(&self) -> bool {
        self.origin_and_importance & 0b1000_0000 > 0
    }

    pub fn specificity(&self) -> Specificity {
        self.specificity
    }

    pub fn stylesheet_index(&self) -> u16 {
        self.stylesheet_index
    }

    pub fn declaration_index(&self) -> u16 {
        self.rule_index
    }
}

#[test]
fn rule_rank_info() {
    macro_rules! create_and_check_roundtrip {
        ($name: ident, $origin: expr, $important: literal, $specificity: tt) => {
            let $name = RuleRankInfo::new($origin, $important, Specificity $specificity, 0, 0);
            assert_eq!($name.origin(), $origin);
            assert_eq!($name.important(), $important);
            assert_eq!($name.specificity(), Specificity $specificity);
        };
    }

    create_and_check_roundtrip!(rank1, Origin::Author, true, (1, 2, 3));
    create_and_check_roundtrip!(rank2, Origin::User, false, (3, 2, 1));
    create_and_check_roundtrip!(rank3, Origin::User, false, (3, 3, 0));

    assert!(rank2 < rank1);
    assert!(rank2 < rank3);

    create_and_check_roundtrip!(rank4, Origin::Author, true, (0, 0, 0));
    create_and_check_roundtrip!(rank5, Origin::User, true, (0, 0, 0));
    create_and_check_roundtrip!(rank6, Origin::UserAgent, true, (0, 0, 0));

    assert!(rank6 > rank5);
    assert!(rank6 > rank4);
    assert!(rank5 > rank4);

    create_and_check_roundtrip!(rank7, Origin::Author, false, (0, 0, 0));
    create_and_check_roundtrip!(rank8, Origin::User, false, (0, 0, 0));
    create_and_check_roundtrip!(rank9, Origin::UserAgent, false, (0, 0, 0));

    assert!(rank9 < rank8);
    assert!(rank8 < rank7);
    assert!(rank9 < rank7);
}

// NOTE: Ruly style declarations are always mutable
//       The restyle CANNOT assume that they have not changed, only
//       selectors are treated as immutable.
//
//       This means all ComputedStyle-level optimisations (like style sharing)
//       must only happen in one restyle pass.
//       For nothing like that is implemented but this is still true.
//
// TODO: This could become a RwLock instead and then we could be like servo and
//       parallelize stuff...
#[derive(Debug, Clone)]
pub struct RuleStyle(Rc<RefCell<DeclarationMap>>);

impl Deref for RuleStyle {
    type Target = RefCell<DeclarationMap>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for RuleStyle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for RuleStyle {}

impl Hash for RuleStyle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Rule {
    pub(super) selectors: Vec<Selector>,
    pub declarations: RuleStyle,
}

impl Rule {
    pub fn new(selectors: Vec<Selector>, declarations: DeclarationMap) -> Self {
        Self {
            selectors,
            declarations: RuleStyle(Rc::new(RefCell::new(declarations))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stylesheet {
    pub origin: Origin,
    pub rules: Vec<Rule>,
}

impl Stylesheet {
    pub fn new(origin: Origin) -> Self {
        Self {
            origin,
            rules: Vec::new(),
        }
    }

    pub fn add(&mut self, rule: Rule) {
        self.rules.push(rule)
    }
}
