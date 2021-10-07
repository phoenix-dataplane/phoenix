use libkoala::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    koala_register().expect("register failed");

    let mask = QpAttrMask;
    let qp_attr = query_qp(mask).unwrap();
}
