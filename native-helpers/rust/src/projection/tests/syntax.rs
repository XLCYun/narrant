/// 只提取字段的类型结构；泛型嵌套可见，数组长度、常量泛型和宏 token 中的类型不混入。
#[test]
fn field_types_walk_nested_types_without_entering_executable_code() {
    use ::syntax::SyntaxKind;
    let document = super::test_code::tests::document(
        r#"
struct Fields {
    nested: Option<Vec<(one::Item, &'static two::Item, *const Other)>>,
    array: [Element; { core::mem::size_of::<NotAFieldType>() }],
    constant: Buffer<Element, { compute::<NotAFieldType>() }>,
    callback: fn(Input) -> Output,
    opaque: make_type!(NotAFieldType),
}
struct Tuple(Slice, &'static [Element]);
enum Choice { Named { item: Item }, Tuple(Item) }
union Storage { item: Item }
fn run(_: Parameter) -> Returned { let _: Local; }
type Alias = Underlying;
const VALUE: ConstantType = make();
"#,
        false,
    );
    let mut names = Vec::new();
    for node in document.tree.descendants() {
        let found = super::syntax::field_type_names(&node);
        if !matches!(
            node.kind(),
            SyntaxKind::RECORD_FIELD | SyntaxKind::TUPLE_FIELD
        ) {
            assert!(found.is_empty());
        }
        names.extend(found.iter().map(|name| name.text().to_string()));
    }
    assert_eq!(
        names,
        [
            "Option", "Vec", "Item", "Item", "Other", "Element", "Buffer", "Element", "Input",
            "Output", "Slice", "Element", "Item", "Item", "Item"
        ]
    );
}
