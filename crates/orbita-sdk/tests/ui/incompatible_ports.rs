use orbita_sdk::{Node, Schema, Version, WorkflowBuilder};

#[derive(Schema)]
#[schema(name = "acme.order", version = 1)]
struct Order {
    id: u64,
}

#[derive(Schema)]
#[schema(name = "acme.customer", version = 1)]
struct Customer {
    id: u64,
}

struct CustomerNode;

impl Node for CustomerNode {
    type Input = Customer;
    type Output = Customer;

    const NAME: &'static str = "acme.customer/load";
    const VERSION: &'static str = "1.0.0";
}

fn main() {
    let mut builder = WorkflowBuilder::new("incompatible", Version::new(1, 0, 0));
    let order = builder.input::<Order>("order").unwrap();
    let customer = builder.node::<CustomerNode>("customer").unwrap();

    builder.connect(order, customer.input());
}
