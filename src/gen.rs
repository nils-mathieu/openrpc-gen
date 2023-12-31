//! Contains the code that actually generates the Rust code.

use std::borrow::Cow;
use std::io;

use convert_case::{Case, Casing};
use open_rpc::ParamStructure;

use crate::parse::{EnumTag, TypeDef, TypeKind, TypeRef};

/// Contains the state of the generator.
struct Ctx<'a> {
    /// The file that is being generated.
    ///
    /// This is not a standard File type but our representation of the generated Rust file.
    pub file: &'a crate::parse::File,
    /// The configuration used to generate the file.
    pub config: &'a crate::config::Config,
}

impl<'a> Ctx<'a> {
    /// Returns the name of the type referenced by the provided [`TypeRef`].
    pub fn type_ref_name(&self, r: &'a TypeRef, required: bool) -> Cow<'a, str> {
        if !required {
            let inner = self.type_ref_name(r, true);
            return Cow::Owned(self.config.primitives.optional.replace("{}", &inner));
        }

        match r {
            TypeRef::Array(inner) => Cow::Owned(
                self.config
                    .primitives
                    .array
                    .replace("{}", &self.type_ref_name(inner, true)),
            ),
            TypeRef::Boolean => Cow::Borrowed(&self.config.primitives.boolean),
            TypeRef::Integer { .. } => Cow::Borrowed(&self.config.primitives.integer),
            TypeRef::Null => Cow::Borrowed(&self.config.primitives.null),
            TypeRef::Number => Cow::Borrowed(&self.config.primitives.number),
            TypeRef::String => Cow::Borrowed(&self.config.primitives.string),
            TypeRef::Keyword(val) => {
                Cow::Owned(format!("{} /* {} */", &self.config.primitives.string, val))
            }
            TypeRef::Ref(path) => match self.file.types.get(path) {
                Some(ty) => Cow::Borrowed(&ty.name),
                None => Cow::Owned(format!("BrokenReference /* {path} */")),
            },
            TypeRef::ExternalRef(name) => Cow::Borrowed(name),
        }
    }
}

/// Generates a Rust file from the provided [`crate::parse::File`] and configuration.
pub fn gen(
    w: &mut dyn io::Write,
    file: &crate::parse::File,
    config: &crate::config::Config,
) -> io::Result<()> {
    let mut ctx = Ctx { file, config };

    writeln!(
        w,
        "\
        //\n\
        // This file was automatically generated by openrpc-gen.\n\
        //\n\
        // Do not edit it manually and instead edit either the source OpenRPC document,\n\
        // the configuration file, or open an issue or pull request on the openrpc-gen\n\
        // GitHub repository.\n\
        // \n\
        //     https://github.com/nils-mathieu/openrpc-gen\n\
        //\n\
        "
    )?;

    writeln!(w, "use serde::{{Serialize, Deserialize}};")?;
    if ctx.config.generation.param_types && !ctx.file.methods.is_empty() {
        writeln!(w, "use serde::ser::SerializeMap;")?;
    }
    for import in &ctx.config.generation.additional_imports {
        writeln!(w, "use {import};")?;
    }
    writeln!(w)?;

    for ty in file.types.values() {
        gen_type(w, &mut ctx, ty)?;
    }
    for method in &file.methods {
        gen_method(w, &mut ctx, method)?;
    }

    Ok(())
}

/// Writes the provided type.
fn gen_type(w: &mut dyn io::Write, ctx: &mut Ctx, ty: &TypeDef) -> io::Result<()> {
    if ctx.config.debug_path {
        writeln!(w, "// {}", ty.path)?;
    }
    if let Some(doc) = &ty.documentation {
        writeln!(w, "/// {}", doc)?;
    }
    match &ty.kind {
        TypeKind::Alias(alias) => {
            writeln!(
                w,
                "pub type {} = {};",
                ty.name,
                ctx.type_ref_name(&alias.ty, true)
            )?;
        }
        TypeKind::Struct(s) => {
            writeln!(w, "#[derive(Debug, Clone, Serialize, Deserialize)]")?;
            writeln!(w, "pub struct {} {{", ty.name)?;
            for field in s.fields.values() {
                if ctx.config.debug_path {
                    writeln!(w, "    // {}", field.path)?;
                }
                if let Some(doc) = &field.documentation {
                    writeln!(w, "    /// {}", doc)?;
                }
                let name = ctx.type_ref_name(&field.ty, field.required);
                if !field.required {
                    writeln!(w, "    #[serde(default)]")?;
                }
                if field.flatten {
                    writeln!(w, "    #[serde(flatten)]")?;
                }
                if field.name != field.name_in_json {
                    writeln!(w, "    #[serde(rename = \"{}\")]", field.name_in_json)?;
                }
                for attr in field.ty.attributes(ctx.config, ctx.file) {
                    writeln!(w, "    {}", attr)?;
                }
                writeln!(w, "    pub {}: {},", field.name, name)?;
            }
            writeln!(w, "}}")?;
        }
        TypeKind::Enum(e) => {
            writeln!(w, "#[derive(Serialize, Deserialize)]")?;
            if e.copy {
                writeln!(w, "#[derive(Copy, PartialEq, Eq, Hash)]")?;
            }
            for global_derive in &ctx.config.generation.global_derives {
                writeln!(w, "#[derive({global_derive})]")?;
            }
            if let Some(derives) = ctx.config.generation.derives.get(&*ty.path) {
                for derive in derives {
                    writeln!(w, "#[derive({derive})]")?;
                }
            }
            match &e.tag {
                EnumTag::Normal => (),
                EnumTag::Tagged(tag) => {
                    writeln!(w, "#[serde(tag = \"{}\")]", tag)?;
                }
                EnumTag::Untagged => {
                    writeln!(w, "#[serde(untagged)]")?;
                }
            }
            writeln!(w, "pub enum {} {{", ty.name)?;
            for variant in e.variants.values() {
                if ctx.config.debug_path {
                    writeln!(w, "    // {}", variant.path)?;
                }
                if let Some(doc) = &variant.documentation {
                    writeln!(w, "    /// {}", doc)?;
                }
                if let Some(name_in_json) = &variant.name_in_json {
                    if name_in_json != &variant.name {
                        writeln!(w, "    #[serde(rename = \"{}\")]", name_in_json)?;
                    }
                }
                if let Some(inner) = &variant.ty {
                    writeln!(
                        w,
                        "    {}({}),",
                        variant.name,
                        ctx.type_ref_name(inner, true)
                    )?;
                } else {
                    writeln!(w, "    {},", variant.name)?;
                }
            }
            writeln!(w, "}}")?;
        }
    }
    writeln!(w)?;

    Ok(())
}

fn gen_method(
    w: &mut dyn io::Write,
    ctx: &mut Ctx,
    method: &crate::parse::Method,
) -> io::Result<()> {
    let std_mod = if ctx.config.generation.use_core {
        "core"
    } else {
        "std"
    };

    let ident_base = if let Some(ref prefix) = ctx.config.generation.method_name_prefix {
        method.name.strip_prefix(prefix).unwrap_or(&method.name)
    } else {
        &method.name
    };

    if ctx.config.generation.method_name_constants {
        writeln!(w, "/// `{}`", method.name)?;
        writeln!(
            w,
            "pub const {}: &str = \"{}\";",
            ident_base.to_case(Case::ScreamingSnake),
            method.name
        )?;
        writeln!(w)?;
    }

    if ctx.config.generation.result_types {
        let mut ident = ident_base.to_case(Case::Pascal);
        ident.push_str("Result");
        if let Some(ref result) = method.result {
            if let Some(ref doc) = result.documentation {
                writeln!(w, "/// {doc}")?;
                writeln!(w, "///")?;
            }
            writeln!(w, "/// Result type of `{}`.", method.name)?;
            writeln!(
                w,
                "pub type {} = {};",
                ident,
                ctx.type_ref_name(&result.ty, true)
            )?;
            writeln!(w)?;
        } else {
            writeln!(
                w,
                "/// Result type of `{}`. This method does not return anything.",
                method.name
            )?;
            writeln!(w, "pub type {} = ();", ident_base.to_case(Case::Pascal))?;
            writeln!(w)?;
        }
    }

    if ctx.config.generation.param_types {
        let mut ident = ident_base.to_case(Case::Pascal);
        ident.push_str("Params");

        writeln!(w, "/// Parameters of the `{}` method.", method.name)?;
        writeln!(w, "#[derive(Debug, Clone)]")?;
        writeln!(w, "pub struct {} {{", ident)?;
        for param in &method.params {
            if let Some(ref doc) = param.documentation {
                writeln!(w, "    /// {doc}")?;
            }
            let param_ident = ctx.type_ref_name(&param.ty, param.required);
            writeln!(w, "    pub {}: {},", param.name, param_ident)?;
        }
        writeln!(w, "}}")?;
        writeln!(w)?;

        writeln!(w, "impl Serialize for {ident} {{")?;
        writeln!(w, "        #[allow(unused_mut)]")?;
        writeln!(
            w,
            "    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>"
        )?;
        writeln!(w, "    where")?;
        writeln!(w, "        S: serde::Serializer,")?;
        writeln!(w, "    {{")?;

        if matches!(
            method.param_structure,
            ParamStructure::ByName | ParamStructure::Either
        ) {
            writeln!(w, "        let mut map = serializer.serialize_map(None)?;")?;
            for param in &method.params {
                writeln!(
                    w,
                    "        map.serialize_entry(\"{}\", &self.{})?;",
                    param.name_in_json, param.name
                )?;
            }
            writeln!(w, "        map.end()")?;
        } else {
            writeln!(w, "        let mut seq = serializer.serialize_seq(None)?;")?;
            for param in &method.params {
                writeln!(w, "        seq.serialize_element(&self.{})?;", param.name)?;
            }
            writeln!(w, "        seq.end()")?;
        }

        writeln!(w, "    }}")?;
        writeln!(w, "}}")?;
        writeln!(w)?;

        writeln!(w, "impl<'de> Deserialize<'de> for {ident} {{")?;
        writeln!(
            w,
            "    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>"
        )?;
        writeln!(w, "    where")?;
        writeln!(w, "        D: serde::Deserializer<'de>,")?;
        writeln!(w, "    {{")?;

        writeln!(w, "        struct Visitor;")?;
        writeln!(w)?;
        writeln!(
            w,
            "        impl<'de> serde::de::Visitor<'de> for Visitor {{"
        )?;
        writeln!(w, "            type Value = {ident};",)?;
        writeln!(w)?;
        writeln!(
            w,
            "            fn expecting(&self, f: &mut {std_mod}::fmt::Formatter) -> {std_mod}::fmt::Result {{"
        )?;
        writeln!(
            w,
            "                write!(f, \"the parameters for `{}`\")",
            method.name
        )?;
        writeln!(w, "            }}")?;
        writeln!(w)?;

        if matches!(
            method.param_structure,
            ParamStructure::ByPosition | ParamStructure::Either
        ) {
            writeln!(w, "            #[allow(unused_mut)]")?;
            writeln!(
                w,
                "            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>"
            )?;
            writeln!(w, "            where")?;
            writeln!(w, "                A: serde::de::SeqAccess<'de>,")?;
            writeln!(w, "            {{")?;
            for (i, param) in method.params.iter().enumerate() {
                writeln!(
                    w,
                    "                let {}: {} = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length({}, &\"expected {} parameters\"))?;",
                    param.name, ctx.type_ref_name(&param.ty, param.required), i + 1, method.params.len(),
                )?;
            }
            writeln!(w)?;
            writeln!(
                w,
                "                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {{"
            )?;
            writeln!(w, "                    return Err(serde::de::Error::invalid_length({}, &\"expected {} parameters\"));", method.params.len() + 1, method.params.len())?;
            writeln!(w, "                }}")?;
            writeln!(w)?;
            writeln!(w, "                Ok({ident} {{")?;
            for param in &method.params {
                writeln!(w, "                    {},", param.name)?;
            }
            writeln!(w, "                }})")?;
            writeln!(w, "            }}")?;
            writeln!(w)?;
        }

        if matches!(
            method.param_structure,
            ParamStructure::ByName | ParamStructure::Either
        ) {
            writeln!(w, "            #[allow(unused_variables)]")?;
            writeln!(
                w,
                "            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>"
            )?;
            writeln!(w, "            where")?;
            writeln!(w, "                A: serde::de::MapAccess<'de>,")?;
            writeln!(w, "            {{")?;
            writeln!(w, "                #[derive(Deserialize)]")?;
            writeln!(w, "                struct Helper {{")?;
            for param in &method.params {
                if !param.required {
                    writeln!(w, "                        #[serde(default)]")?;
                }
                writeln!(
                    w,
                    "                    {}: {},",
                    param.name,
                    ctx.type_ref_name(&param.ty, param.required)
                )?;
            }
            writeln!(w, "                }}")?;
            writeln!(w)?;
            writeln!(w, "                let helper = Helper::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;")?;
            writeln!(w)?;
            writeln!(w, "                Ok({ident} {{")?;
            for param in &method.params {
                writeln!(
                    w,
                    "                    {}: helper.{},",
                    param.name, param.name
                )?;
            }
            writeln!(w, "                }})")?;
            writeln!(w, "            }}")?;
            writeln!(w)?;
        }

        writeln!(w, "        }}")?;
        writeln!(w)?;

        match method.param_structure {
            ParamStructure::ByName => {
                writeln!(w, "        deserializer.deserialize_map(Visitor)")?;
            }
            ParamStructure::ByPosition => {
                writeln!(w, "        deserializer.deserialize_seq(Visitor)")?;
            }
            ParamStructure::Either => {
                writeln!(w, "        deserializer.deserialize_any(Visitor)")?;
            }
        }

        writeln!(w, "    }}")?;
        writeln!(w, "}}")?;
        writeln!(w)?;
    }

    Ok(())
}
