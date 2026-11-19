//! Nested pane layout. IDs survive sibling removal and divider resizing.

use super::Split;

pub(in crate::app) struct Pane {
    pub(in crate::app) id: usize,
    pub(super) selected: Option<u64>,
}

pub(in crate::app) enum PaneNode {
    Leaf(Pane),
    Split {
        id: usize,
        direction: Split,
        fraction: f32,
        children: [Box<PaneNode>; 2],
    },
}

impl PaneNode {
    pub(super) fn leaf(id: usize, selected: Option<u64>) -> Self {
        Self::Leaf(Pane { id, selected })
    }

    pub(super) fn pane(&self, id: usize) -> Option<&Pane> {
        match self {
            Self::Leaf(pane) => (pane.id == id).then_some(pane),
            Self::Split { children, .. } => children[0].pane(id).or_else(|| children[1].pane(id)),
        }
    }

    pub(super) fn pane_mut(&mut self, id: usize) -> Option<&mut Pane> {
        match self {
            Self::Leaf(pane) => (pane.id == id).then_some(pane),
            Self::Split { children, .. } => {
                let [first, second] = children;
                first.pane_mut(id).or_else(|| second.pane_mut(id))
            }
        }
    }

    pub(super) fn ids(&self) -> Vec<usize> {
        let mut ids = Vec::new();
        self.collect_ids(&mut ids);
        ids
    }

    fn collect_ids(&self, ids: &mut Vec<usize>) {
        match self {
            Self::Leaf(pane) => ids.push(pane.id),
            Self::Split { children, .. } => {
                for child in children {
                    child.collect_ids(ids);
                }
            }
        }
    }

    pub(super) fn split_pane(
        &mut self,
        pane: usize,
        new_pane: usize,
        divider: usize,
        direction: Split,
    ) -> bool {
        match self {
            Self::Leaf(current) if current.id == pane => {
                let original = std::mem::replace(self, Self::leaf(new_pane, None));
                *self = Self::Split {
                    id: divider,
                    direction,
                    fraction: 0.5,
                    children: [Box::new(original), Box::new(Self::leaf(new_pane, None))],
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { children, .. } => children
                .iter_mut()
                .any(|child| child.split_pane(pane, new_pane, divider, direction)),
        }
    }

    pub(super) fn set_fraction(&mut self, divider: usize, value: f32) {
        if let Self::Split {
            id,
            fraction,
            children,
            ..
        } = self
        {
            if *id == divider {
                *fraction = value.clamp(0.0, 1.0);
            } else {
                for child in children {
                    child.set_fraction(divider, value);
                }
            }
        }
    }

    pub(super) fn retain(self, occupied: &impl Fn(usize) -> bool) -> Option<Self> {
        match self {
            Self::Leaf(ref pane) => occupied(pane.id).then_some(self),
            Self::Split {
                id,
                direction,
                fraction,
                children,
            } => {
                let [first, second] = children;
                match (first.retain(occupied), second.retain(occupied)) {
                    (Some(first), Some(second)) => Some(Self::Split {
                        id,
                        direction,
                        fraction,
                        children: [Box::new(first), Box::new(second)],
                    }),
                    (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                    (None, None) => None,
                }
            }
        }
    }
}
