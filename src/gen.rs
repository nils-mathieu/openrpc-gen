//! Contains the code that actually generates the Rust code.

use std::borrow::Cow;
use std::io;

use convert_case::{Case, Casing};

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
    pub fn type_ref_name(&self, r: &'a TypeRef) -> Cow<'a, str> {
        match r {
            TypeRef::Array(inner) => Cow::Owned(
                self.config
                    .primitives
                    .array
                    .replace("{}", &self.type_ref_name(inner)),
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
                ctx.type_ref_name(&alias.ty)
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
                let mut name = ctx.type_ref_name(&field.ty);
                if !field.required {
                    writeln!(w, "    #[serde(default)]")?;
                    name = Cow::Owned(ctx.config.primitives.optional.replace("{}", &name));
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
                    writeln!(w, "    {}({}),", variant.name, ctx.type_ref_name(inner))?;
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
            writeln!(w, "pub type {} = {};", ident, ctx.type_ref_name(&result.ty))?;
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
        writeln!(w, "#[derive(Debug, Clone, Serialize, Deserialize)]")?;
        writeln!(w, "pub struct {} {{", ident)?;
        for param in &method.params {
            if let Some(ref doc) = param.documentation {
                writeln!(w, "    /// {doc}")?;
            }
            writeln!(
                w,
                "    pub {}: {},",
                param.name,
                ctx.type_ref_name(&param.ty)
            )?;
        }
        writeln!(w, "}}")?;
        writeln!(w)?;
    }

    Ok(())
}
