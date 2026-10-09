//! Minimal process-owned AT-SPI exposure for the native desktop window.

use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Role,
    TreeId, TreeInfo, TreeUpdate,
};

struct Activation;

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let root = NodeId::from(0u64);
        let mut node = Node::new(Role::Window);
        node.set_label(crate::config::APP_NAME);
        Some(TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        })
    }
}

struct NoopAction;

impl ActionHandler for NoopAction {
    fn do_action(&mut self, _request: ActionRequest) {}
}

struct NoopDeactivation;

impl DeactivationHandler for NoopDeactivation {
    fn deactivate_accessibility(&mut self) {}
}

pub struct DesktopAccessibility {
    _adapter: accesskit_unix::Adapter,
}

impl DesktopAccessibility {
    pub fn new() -> Self {
        Self {
            _adapter: accesskit_unix::Adapter::new(Activation, NoopAction, NoopDeactivation),
        }
    }
}
