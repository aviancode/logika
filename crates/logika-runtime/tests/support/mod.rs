use std::collections::BTreeMap;

use logika_core::{PrimitiveType, SchemaDefinition, TypeRef};
use logika_workflow::{
    CompilationOptions, CompilationResolver, NodeInterface, NodeReference, TypeReference,
    ValidationResolver, WorkflowDocument, WorkflowMetadata, WorkflowSpec, compile_workflow,
};
use semver::Version;

struct Resolver {
    payload: TypeRef,
}

impl ValidationResolver for Resolver {
    fn resolve_node(&self, _reference: &NodeReference) -> Option<&NodeInterface> {
        None
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        (reference.name() == self.payload.name() && reference.version() == self.payload.version())
            .then_some(&self.payload)
    }
}

impl CompilationResolver for Resolver {
    fn resolve_node_version(&self, _reference: &NodeReference) -> Option<&Version> {
        None
    }
}

pub fn plan() -> logika_workflow::ExecutionPlan {
    let payload = TypeRef::new(
        "test.payload",
        1,
        SchemaDefinition::Primitive(PrimitiveType::String),
    )
    .expect("test schema must be canonical");
    let input = "test.payload@1"
        .parse::<TypeReference>()
        .expect("test reference must be valid");
    let input_port = "input".parse().expect("test port must be valid");
    let output_port = "output".parse().expect("test port must be valid");
    let document = WorkflowDocument::new(
        WorkflowMetadata::new("runtime-model", Version::new(2, 3, 4)),
        WorkflowSpec::new(
            BTreeMap::from([(input_port, input)]),
            Vec::new(),
            Vec::new(),
            BTreeMap::from([(
                output_port,
                logika_workflow::Endpoint::workflow_input(
                    "input".parse().expect("test port must be valid"),
                ),
            )]),
        ),
    );

    compile_workflow(
        &document,
        &Resolver { payload },
        CompilationOptions::default(),
    )
    .expect("test workflow must compile")
}
