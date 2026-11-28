//! RS-11 `Rc<RefCell>` object graphs, cured with an index arena; RS-24 layout.

use std::num::NonZeroU32;

/// A handle to a node, valid only for the [`Tree`] that issued it.
///
/// Handles are plain indices. A handle from another tree that happens to be in
/// range is not detected; use a generational arena when handles can cross trees
/// or outlive removals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    fn index(self) -> usize {
        // On targets where `usize` is narrower than `u32`, an index that does not
        // fit maps to `usize::MAX`, which no vector reaches.
        usize::try_from(self.0.get() - 1).unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    parent: Option<NodeId>,
    weight: f32,
}

/// Why a node could not be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeError {
    /// The parent handle is out of range for this tree.
    NoSuchParent,
    /// The tree already holds `u32::MAX` nodes.
    Full,
}

/// A tree stored in one contiguous vector and linked by [`NodeId`] handles.
#[derive(Debug, Default)]
pub struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// An empty tree with room for `capacity` nodes.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity),
        }
    }

    /// Adds a node under `parent`, or as a root, and returns its handle.
    ///
    /// # Errors
    /// [`TreeError::NoSuchParent`] if `parent` is out of range;
    /// [`TreeError::Full`] if no further handle can be issued.
    pub fn insert(&mut self, parent: Option<NodeId>, weight: f32) -> Result<NodeId, TreeError> {
        let id = u32::try_from(self.nodes.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .and_then(NonZeroU32::new)
            .map(NodeId)
            .ok_or(TreeError::Full)?;
        let next_sibling = match parent {
            Some(p) => {
                let parent_node = self
                    .nodes
                    .get_mut(p.index())
                    .ok_or(TreeError::NoSuchParent)?;
                parent_node.first_child.replace(id)
            }
            None => None,
        };
        self.nodes.push(Node {
            first_child: None,
            next_sibling,
            parent,
            weight,
        });
        Ok(id)
    }

    /// The parent of `id`, if it has one.
    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.index()).and_then(|n| n.parent)
    }

    /// The children of `id`, most recently added first.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> {
        let first = self.nodes.get(id.index()).and_then(|n| n.first_child);
        std::iter::successors(first, |child| {
            self.nodes.get(child.index()).and_then(|n| n.next_sibling)
        })
    }

    /// The sum of every node's weight, as one sequential scan.
    ///
    /// The scan is cache-friendly. It is not vectorized: floating-point addition
    /// is not associative, so the compiler keeps the order as written.
    #[must_use]
    pub fn total_weight(&self) -> f32 {
        self.nodes.iter().map(|n| n.weight).sum()
    }

    /// The number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_use_the_zero_niche() {
        assert_eq!(
            size_of::<Option<NodeId>>(),
            4,
            "None fits in the zero niche"
        );
        assert_eq!(size_of::<Node>(), 16, "three links and a weight");
    }

    #[test]
    fn children_and_parents_link_up() {
        let mut tree = Tree::with_capacity(3);
        assert!(tree.nodes.capacity() >= 3, "room is reserved up front");
        let root = tree.insert(None, 1.0).unwrap();
        let a = tree.insert(Some(root), 2.0).unwrap();
        let b = tree.insert(Some(root), 3.0).unwrap();
        assert_eq!(
            tree.children(root).collect::<Vec<_>>(),
            vec![b, a],
            "newest first"
        );
        assert_eq!(tree.parent(a), Some(root), "child knows its parent");
        assert_eq!(tree.total_weight(), 6.0, "weights sum");
        assert_eq!(tree.len(), 3, "three nodes");
        assert!(!tree.is_empty(), "a tree with nodes is not empty");
    }

    #[test]
    fn an_out_of_range_parent_is_refused() {
        let mut tree = Tree::default();
        let stray = NodeId(NonZeroU32::new(9).unwrap());
        assert_eq!(
            tree.insert(Some(stray), 0.0),
            Err(TreeError::NoSuchParent),
            "no node 9"
        );
        assert!(tree.is_empty(), "nothing was added");
    }
}
