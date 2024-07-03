use convert_case::{Case, Casing};
use petgraph::algo::has_path_connecting;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::{algo, Direction};
use std::collections::hash_map::Keys;
use std::collections::HashMap;

use crate::parse::{TypeKind, TypeRef};

pub(crate) struct TypeDeps {
    g: DiGraph<String, ()>,
    nodes: HashMap<String, NodeIndex<u32>>,
}

impl TypeDeps {
    pub(crate) fn new() -> TypeDeps {
        TypeDeps {
            g: Default::default(),
            nodes: Default::default(),
        }
    }

    pub(crate) fn add(&mut self, config: &crate::config::Config, file: &crate::parse::File) {
        fn lift_out<'a>(
            file: &'a crate::parse::File,
            config: &'a crate::config::Config,
            r: &'a TypeRef,
        ) -> &'a str {
            match r {
                TypeRef::Array(inner) => lift_out(file, config, inner),
                TypeRef::Boolean => &config.primitives.boolean,
                TypeRef::Integer { .. } => &config.primitives.integer,
                TypeRef::Null => &config.primitives.null,
                TypeRef::Number => &config.primitives.number,
                TypeRef::String => &config.primitives.string,
                TypeRef::Keyword(_) => &config.primitives.string.as_str(),
                TypeRef::Ref(path) => match file.types.get(path) {
                    Some(ty) => &ty.name,
                    None => "BrokenReference",
                },
                TypeRef::ExternalRef(name) => name,
            }
        }

        fn add_dep(
            deps: &mut TypeDeps,
            file: &crate::parse::File,
            config: &crate::config::Config,
            a: String,
            b: &TypeRef,
            required: bool,
        ) {
            let b = String::from(lift_out(file, config, b));
            deps.add_edge(a.clone(), b.clone());
            if !required {
                deps.add_edge(a.clone(), "_MaybeImplicitDefault".into());
                deps.add_edge(b.clone(), "_Optional".into());
            }
        }

        for ty in file.types.values() {
            match &ty.kind {
                TypeKind::Alias(alias) => {
                    add_dep(self, file, config, ty.name.clone(), &alias.ty, true)
                }
                TypeKind::Struct(s) => {
                    for field in s.fields.values() {
                        add_dep(
                            self,
                            file,
                            config,
                            ty.name.clone(),
                            &&field.ty,
                            field.required,
                        );
                    }
                }
                TypeKind::Enum(e) => {
                    for variant in e.variants.values() {
                        if let Some(inner) = &variant.ty {
                            add_dep(self, file, config, ty.name.clone(), inner, true);
                        }
                    }
                }
            }
        }

        for method in &file.methods {
            let ident_base = if let Some(ref prefix) = config.generation.method_name_prefix {
                method.name.strip_prefix(prefix).unwrap_or(&method.name)
            } else {
                &method.name
            };

            if config.generation.param_types {
                for param in &method.params {
                    add_dep(
                        self,
                        file,
                        config,
                        format!("{}Params", ident_base.to_case(Case::Pascal)),
                        &param.ty,
                        param.required,
                    );
                }
            }
        }
    }

    pub(crate) fn has_path(&self, a: &String, b: &String) -> bool {
        if a == b {
            return false;
        }
        if !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return false;
        }
        let a_node = self.nodes.get(a).unwrap();
        let b_node = self.nodes.get(b).unwrap();
        algo::has_path_connecting(&self.g, *a_node, *b_node, None)
    }

    pub(crate) fn has_indirect_path(&self, a: &String, b: &String) -> bool {
        if !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return false;
        }
        let a = self.nodes.get(a).unwrap();
        let b = self.nodes.get(b).unwrap();

        algo::all_simple_paths::<Vec<_>, _>(&self.g, *a, *b, 1, None)
            .next()
            .is_some()
    }

    pub(crate) fn add_edge(&mut self, a: String, b: String) {
        let a = self.get_node(&a);
        let b = self.get_node(&b);
        self.g.add_edge(a, b, ());
    }

    pub(crate) fn fix_defaults(&mut self) {
        let maybe_implicit_default = self.get_node(&"_MaybeImplicitDefault".into());
        let f = self.get_node(&String::from("F"));
        let d = self.get_node(&format!("{}_ImplicitDefault", "F"));
        let optional = self.get_node(&String::from("_Optional"));
        let edges = self
            .g
            .edges_directed(maybe_implicit_default, Direction::Incoming)
            .filter(|e| {
                self.g
                    .edges(e.source())
                    .filter(|e| {
                        self.g.contains_edge(e.target(), optional)
                            && has_path_connecting(&self.g, e.target(), f, None)
                    })
                    .count()
                    > 0
            })
            .map(|e| e.source())
            .collect::<Vec<_>>();
        for e in edges {
            self.g.add_edge(e, d, ());
        }
    }

    fn get_node(&mut self, n: &String) -> NodeIndex {
        match self.nodes.get(n) {
            Some(i) => *i,
            None => {
                let i = self.g.add_node(n.clone());
                self.nodes.insert(n.to_string(), i);
                i
            }
        }
    }

    pub(crate) fn debug(&mut self, a: &String, b: &String) {
        let a_node = self.get_node(a);
        let b_node = self.get_node(b);
        let ways = algo::all_simple_paths::<Vec<_>, _>(&self.g, a_node, b_node, 0, None)
            .collect::<Vec<_>>();

        if ways.is_empty() {
            println!("No {a} -> {b} path found");
            return;
        }
        println!("{a} -> {b} path:");
        for n in ways.first().unwrap() {
            println!("{:?}", self.g.node_weight(*n));
        }
    }
}
