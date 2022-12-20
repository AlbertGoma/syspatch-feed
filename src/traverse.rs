use html5ever::tendril::StrTendril;
use markup5ever_rcdom::NodeData::{Element, Text};
use markup5ever_rcdom::{Handle, Node, NodeData};

pub(crate) trait Traverse {
    fn first_child_by_name(&self, name: &str) -> Option<Handle>;
    fn children_by_name(&self, name: &str, list: Vec<Handle>) -> Vec<Handle>;
    fn get_first_text(&self) -> String;
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

    fn children_by_name(&self, child_name: &str, mut list: Vec<Handle>) -> Vec<Handle> {
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
                text += &*contents.borrow().to_string();
                return text;
            }
        }
        text
    }
}


pub(crate) trait TraverseAttrs {
    fn first_attr_by_name(&self, name: &str) -> Option<StrTendril>;
}

impl TraverseAttrs for NodeData {
    fn first_attr_by_name(&self, attr_name: &str) -> Option<StrTendril> {
        if let Element {ref attrs, ..} = self {
            for attr in attrs.borrow().as_slice() {
                if attr.name.local.eq_str_ignore_ascii_case(attr_name) {
                    return Some(attr.value.clone())
                }
            }
        }
        None
    }
}
