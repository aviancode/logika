//! In-memory node registry and future plugin registry contracts for `logika`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use logika_core::{Error, ErrorCategory, ErrorDetail, PluginId, Result, TypeRef};
use logika_workflow::{
    CompilationResolver, NodeInterface, NodeReference, TypeReference, ValidationResolver,
};
use semver::Version;

/// Description of one locally registered Rust node implementation.
///
/// A descriptor contains only portable metadata. Runtime execution hooks are
/// intentionally left to the SDK and runtime crates, while validation can use
/// the node's interface without loading a plugin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDescriptor {
    name: PluginId,
    version: Version,
    interface: NodeInterface,
}

impl NodeDescriptor {
    /// Creates a descriptor for a concrete local node version.
    #[must_use]
    pub const fn new(name: PluginId, version: Version, interface: NodeInterface) -> Self {
        Self {
            name,
            version,
            interface,
        }
    }

    /// Returns the globally namespaced node implementation name.
    #[must_use]
    pub const fn name(&self) -> &PluginId {
        &self.name
    }

    /// Returns the concrete semantic version of this implementation.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns the node's validated input and output contract.
    #[must_use]
    pub const fn interface(&self) -> &NodeInterface {
        &self.interface
    }
}

/// Deterministic in-memory registry for local Rust node descriptors.
///
/// Multiple versions of a node may coexist. Resolution selects the highest
/// registered version accepted by the workflow's SemVer requirement. Schemas
/// referenced by registered node ports are indexed automatically, making the
/// registry directly usable as a workflow [`ValidationResolver`].
#[derive(Debug, Default)]
pub struct NodeRegistry {
    nodes: BTreeMap<PluginId, BTreeMap<Version, NodeDescriptor>>,
    types: BTreeMap<String, BTreeMap<u32, TypeRef>>,
    len: usize,
}

impl NodeRegistry {
    /// Creates an empty local registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            types: BTreeMap::new(),
            len: 0,
        }
    }

    /// Registers a concrete local Rust node descriptor.
    ///
    /// An exact name/version pair can be registered only once. Port schemas
    /// sharing the same stable name and schema version must have identical
    /// canonical fingerprints. Validation is completed before the registry is
    /// mutated, so a rejected descriptor leaves it unchanged.
    pub fn register_local(&mut self, descriptor: NodeDescriptor) -> Result<()> {
        if self
            .nodes
            .get(descriptor.name())
            .is_some_and(|versions| versions.contains_key(descriptor.version()))
        {
            return Err(resolution_error(
                "registry.duplicate_node",
                format!(
                    "local node {}@{} is already registered",
                    descriptor.name(),
                    descriptor.version()
                ),
            ));
        }

        let mut pending_types = BTreeMap::<String, BTreeMap<u32, TypeRef>>::new();
        for input in descriptor.interface().inputs().values() {
            self.stage_type(input.type_ref(), &mut pending_types)?;
        }
        for output in descriptor.interface().outputs().values() {
            self.stage_type(output, &mut pending_types)?;
        }

        for (name, versions) in pending_types {
            self.types.entry(name).or_default().extend(versions);
        }
        self.nodes
            .entry(descriptor.name().clone())
            .or_default()
            .insert(descriptor.version().clone(), descriptor);
        self.len += 1;
        Ok(())
    }

    /// Registers a canonical schema not already present on a node port.
    ///
    /// Re-registering the identical schema is idempotent. A different schema
    /// with the same stable name and version is rejected.
    pub fn register_type(&mut self, type_ref: TypeRef) -> Result<()> {
        self.ensure_type_is_compatible(&type_ref, None)?;
        self.types
            .entry(type_ref.name().to_owned())
            .or_default()
            .entry(type_ref.version())
            .or_insert(type_ref);
        Ok(())
    }

    /// Resolves a version-constrained workflow reference.
    ///
    /// The highest compatible registered version is returned.
    pub fn resolve(&self, reference: &NodeReference) -> Result<&NodeDescriptor> {
        let Some(versions) = self.nodes.get(reference.plugin()) else {
            return Err(resolution_error(
                "registry.node_not_found",
                format!("local node {} is not registered", reference.plugin()),
            ));
        };

        self.find(reference).ok_or_else(|| {
            let available = versions
                .keys()
                .map(Version::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            resolution_error(
                "registry.no_matching_version",
                format!(
                    "local node {} has no version matching {}; available versions: [{}]",
                    reference.plugin(),
                    reference.version_requirement(),
                    available
                ),
            )
        })
    }

    /// Looks up one exact local node version.
    #[must_use]
    pub fn get(&self, name: &PluginId, version: &Version) -> Option<&NodeDescriptor> {
        self.nodes.get(name)?.get(version)
    }

    /// Iterates over all descriptors in deterministic name/version order.
    pub fn descriptors(&self) -> impl Iterator<Item = &NodeDescriptor> {
        self.nodes.values().flat_map(|versions| versions.values())
    }

    /// Returns the number of registered concrete node versions.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether no node descriptors are registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn find(&self, reference: &NodeReference) -> Option<&NodeDescriptor> {
        self.nodes
            .get(reference.plugin())?
            .iter()
            .rev()
            .find(|(version, _)| reference.version_requirement().matches(version))
            .map(|(_, descriptor)| descriptor)
    }

    fn stage_type(
        &self,
        type_ref: &TypeRef,
        pending: &mut BTreeMap<String, BTreeMap<u32, TypeRef>>,
    ) -> Result<()> {
        self.ensure_type_is_compatible(type_ref, pending.get(type_ref.name()))?;
        pending
            .entry(type_ref.name().to_owned())
            .or_default()
            .entry(type_ref.version())
            .or_insert_with(|| type_ref.clone());
        Ok(())
    }

    fn ensure_type_is_compatible(
        &self,
        type_ref: &TypeRef,
        pending_versions: Option<&BTreeMap<u32, TypeRef>>,
    ) -> Result<()> {
        let registered = pending_versions
            .and_then(|versions| versions.get(&type_ref.version()))
            .or_else(|| {
                self.types
                    .get(type_ref.name())
                    .and_then(|versions| versions.get(&type_ref.version()))
            });

        if let Some(registered) = registered
            && registered != type_ref
        {
            return Err(Error::new(
                ErrorCategory::Schema,
                ErrorDetail::new(
                    "registry.conflicting_schema",
                    format!(
                        "schema {}@{} conflicts with registered fingerprint {} (candidate {})",
                        type_ref.name(),
                        type_ref.version(),
                        registered.fingerprint(),
                        type_ref.fingerprint()
                    ),
                ),
            ));
        }
        Ok(())
    }
}

impl ValidationResolver for NodeRegistry {
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface> {
        self.find(reference).map(NodeDescriptor::interface)
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        self.types.get(reference.name())?.get(&reference.version())
    }
}

impl CompilationResolver for NodeRegistry {
    fn resolve_node_version(&self, reference: &NodeReference) -> Option<&Version> {
        self.find(reference).map(NodeDescriptor::version)
    }
}

fn resolution_error(code: &'static str, message: String) -> Error {
    Error::new(ErrorCategory::Resolution, ErrorDetail::new(code, message))
}
