use html5ever::tendril::StrTendril;
use markup5ever_rcdom::NodeData::{Element, Text};
use markup5ever_rcdom::{Handle, Node, NodeData};
use std::borrow::Borrow;
use std::collections::HashMap;

pub(crate) trait Traverse {
    fn first_child_by_name(&self, name: &str) -> Option<Handle>;
    fn children_by_name(&self, name: &str) -> Vec<Handle>;
    fn get_first_text(&self) -> String;
    fn index_following_text_by_children_attr(
        &self,
        attr_name: &str,
        key_mod: impl Fn(String) -> String,
        idx: &mut HashMap<String, StrTendril>,
    );
}

impl Traverse for Node {
    fn first_child_by_name(&self, child_name: &str) -> Option<Handle> {
        for child in self.children.borrow().as_slice() {
            if let Element { ref name, .. } = child.data {
                //console_debug!("name.local = {:?}", name.local);
                if name.local.eq_str_ignore_ascii_case(child_name) {
                    //console_debug!("name.local = child_name");
                    return Some(child.clone());
                }
            }
        }
        None
    }

    fn children_by_name(&self, child_name: &str) -> Vec<Handle> {
        let mut list = Vec::<Handle>::new();
        for child in self.children.borrow().as_slice() {
            if let Element { ref name, .. } = child.data {
                //console_debug!("name.local = {:?}", name.local);
                if name.local.eq_str_ignore_ascii_case(child_name) {
                    //console_debug!("name.local = child_name");
                    list.push(child.clone());
                }
            }
        }
        list
    }

    fn get_first_text(&self) -> String {
        let mut text = String::new();
        for child in self.children.borrow().as_slice() {
            if let Text { ref contents } = child.data {
                text += &contents.borrow().to_string();
                return text;
            }
        }
        text
    }

    fn index_following_text_by_children_attr(
        &self,
        attr_name: &str,
        key_mod: impl Fn(String) -> String,
        idx: &mut HashMap<String, StrTendril>,
    ) {
        for (i, child) in self.children.borrow().iter().enumerate() {
            if let Element { ref attrs, .. } = child.data {
                for attr in attrs.borrow().as_slice() {
                    if attr.name.local.eq_str_ignore_ascii_case(attr_name) {
                        if let Some(node) = self.children.borrow().as_slice().get(i + 1).borrow() {
                            if let Text { ref contents } = node.data {
                                idx.insert(
                                    key_mod(attr.value.to_string()),
                                    contents.borrow().clone(),
                                );
                            }
                        }
                        break;
                    }
                }
            }
        }
    }
}

pub(crate) trait TraverseAttrs {
    fn first_attr_by_name(&self, name: &str) -> Option<StrTendril>;
}

impl TraverseAttrs for NodeData {
    fn first_attr_by_name(&self, attr_name: &str) -> Option<StrTendril> {
        if let Element { ref attrs, .. } = self {
            for attr in attrs.borrow().as_slice() {
                if attr.name.local.eq_str_ignore_ascii_case(attr_name) {
                    return Some(attr.value.clone());
                }
            }
        }
        None
    }
}
