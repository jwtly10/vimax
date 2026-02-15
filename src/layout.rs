#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone)]
pub enum LayoutNode {
    Leaf(usize),
    Split {
        direction: SplitDirection,
        #[allow(dead_code)]
        ratio: f32,
        children: [Box<LayoutNode>; 2],
    },
}

// TODO: we should fix these dead codes warnings
// ignoring for now as these currently work - should address when next working here
#[allow(dead_code)]
impl LayoutNode {
    pub fn single(window_id: usize) -> Self {
        LayoutNode::Leaf(window_id)
    }

    pub fn split_leaf(&mut self, target: usize, new_id: usize, direction: SplitDirection) -> bool {
        match self {
            LayoutNode::Leaf(id) if *id == target => {
                let old = Box::new(LayoutNode::Leaf(*id));
                let new = Box::new(LayoutNode::Leaf(new_id));
                *self = LayoutNode::Split {
                    direction,
                    ratio: 0.5,
                    children: [old, new],
                };
                true
            }
            LayoutNode::Split { children, .. } => {
                children[0].split_leaf(target, new_id, direction)
                    || children[1].split_leaf(target, new_id, direction)
            }
            _ => false,
        }
    }

    pub fn remove_leaf(&mut self, target: usize) -> bool {
        match self {
            LayoutNode::Split { children, .. } => {
                if let LayoutNode::Leaf(id) = *children[0]
                    && id == target
                {
                    *self = *children[1].clone();
                    return true;
                }
                if let LayoutNode::Leaf(id) = *children[1]
                    && id == target
                {
                    *self = *children[0].clone();
                    return true;
                }
                children[0].remove_leaf(target) || children[1].remove_leaf(target)
            }
            _ => false,
        }
    }

    pub fn leaf_count(&self) -> usize {
        match self {
            LayoutNode::Leaf(_) => 1,
            LayoutNode::Split { children, .. } => {
                children[0].leaf_count() + children[1].leaf_count()
            }
        }
    }

    pub fn leaves(&self) -> Vec<usize> {
        match self {
            LayoutNode::Leaf(id) => vec![*id],
            LayoutNode::Split { children, .. } => {
                let mut v = children[0].leaves();
                v.extend(children[1].leaves());
                v
            }
        }
    }

    pub fn first_leaf(&self) -> usize {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split { children, .. } => children[0].first_leaf(),
        }
    }

    pub fn neighbor(
        &self,
        current: usize,
        direction: SplitDirection,
        forward: bool,
    ) -> Option<usize> {
        self.find_neighbor(current, direction, forward)
    }

    fn find_neighbor(&self, current: usize, dir: SplitDirection, forward: bool) -> Option<usize> {
        match self {
            LayoutNode::Leaf(_) => None,
            LayoutNode::Split {
                direction,
                children,
                ..
            } => {
                if *direction == dir {
                    let (search_child, target_child) = if forward { (0, 1) } else { (1, 0) };

                    if children[search_child].contains_leaf(current) {
                        if let Some(inner) =
                            children[search_child].find_neighbor(current, dir, forward)
                        {
                            return Some(inner);
                        }
                        return Some(if forward {
                            children[target_child].first_leaf()
                        } else {
                            children[target_child].last_leaf()
                        });
                    }

                    if children[target_child].contains_leaf(current) {
                        return children[target_child].find_neighbor(current, dir, forward);
                    }
                } else {
                    for child in children {
                        if child.contains_leaf(current) {
                            return child.find_neighbor(current, dir, forward);
                        }
                    }
                }
                None
            }
        }
    }

    fn contains_leaf(&self, target: usize) -> bool {
        match self {
            LayoutNode::Leaf(id) => *id == target,
            LayoutNode::Split { children, .. } => {
                children[0].contains_leaf(target) || children[1].contains_leaf(target)
            }
        }
    }

    fn last_leaf(&self) -> usize {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split { children, .. } => children[1].last_leaf(),
        }
    }

    pub fn fix_ids_after_remove(&mut self, removed: usize) {
        match self {
            LayoutNode::Leaf(id) => {
                if *id > removed {
                    *id -= 1;
                }
            }
            LayoutNode::Split { children, .. } => {
                children[0].fix_ids_after_remove(removed);
                children[1].fix_ids_after_remove(removed);
            }
        }
    }
}
