use super::*;

fn error<T: Parse>(tokens: Tokens) -> String {
    match syn::parse2::<T>(tokens) {
        Ok(_) => panic!("expected a syntax error"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn empty_lists_and_explicit_sync_modes() {
    for tokens in [
        quote!([::library]; dispatcher, {}),
        quote!([::library]; dispatcher, sync {}),
    ] {
        let input: Commands = syn::parse2(tokens).unwrap();
        assert_eq!(input.mode, Mode::Sync);
        assert!(input.nodes.is_empty());
    }
    let input: Command = syn::parse2(quote!([::library]; sync; node => {})).unwrap();
    assert_eq!(input.mode, Mode::Sync);
}

#[test]
fn run_is_not_a_reserved_builder_name() {
    for expression in [
        quote!(run),
        quote!(r#run),
        quote!(run::factory()),
        quote!(run.method()),
        quote!(run()),
        quote!(run == other),
    ] {
        let input: Commands = syn::parse2(quote!([::library]; dispatcher, {
            #expression => {};
            literal("parent") => { #expression => {}; };
        }))
        .unwrap();
        assert_eq!(input.nodes.len(), 2);
        assert!(input.nodes[1].handler.is_none());
        assert_eq!(input.nodes[1].children.len(), 1);
    }
}

#[test]
fn arbitrary_rust_expressions_are_not_whitelisted() {
    for expression in [
        quote!({
            side_effect();
            builder
        }),
        quote!(if condition { first } else { second }),
        quote!(match choice {
            Some(node) => node,
            None => fallback,
        }),
        quote!((|name| factory(name))("name")),
        quote!(<Parser as CommandArgument>::arg("name")),
        quote!(factory::<State, Output>()?),
        quote!(tuple.0),
    ] {
        syn::parse2::<Command>(quote!([::library]; #expression => {
            run: |ctx| -> Result<i32, Error> { call(ctx)?; Ok(1) };
        }))
        .unwrap();
    }
}

#[test]
fn misplaced_and_duplicate_handlers_are_distinct() {
    let late = error::<Command>(quote!([::library]; parent => {
        child => {};
        run: handler;
    }));
    assert!(late.contains("must precede child nodes"));
    for tokens in [
        quote!([::library]; parent => { run: first; run: second; }),
        quote!([::library]; parent => { run: first; child => {}; run sync: second; }),
    ] {
        assert!(error::<Command>(tokens).contains("duplicate `run`"));
    }
    assert!(
        error::<Commands>(quote!([::library]; dispatcher, {
            run: handler;
        }))
        .contains("belongs inside a node body")
    );
}

#[test]
fn handler_syntax_explains_the_expected_form() {
    for body in [quote!(run = handler;), quote!(run sync handler;)] {
        assert_eq!(
            error::<Command>(quote!([::library]; node => { #body })),
            HANDLER_SYNTAX,
        );
    }
    for body in [quote!(run:;), quote!(run:), quote!(run sync:;)] {
        assert!(
            error::<Command>(quote!([::library]; node => { #body })).contains("missing handler")
        );
    }
    assert!(
        error::<Command>(quote!([::library]; node => { rn: handler; }))
            .contains("unknown node entry `rn:`")
    );
}

#[test]
fn arrows_and_semicolons_have_context() {
    assert!(error::<Command>(quote!([::library]; node -> {})).contains("node => { ... }"));
    assert!(
        error::<Commands>(quote!([::library]; d, { node => {} }))
            .contains("missing `;` after command node")
    );
    assert!(
        error::<Command>(quote!([::library]; node => { child => {}, }))
            .contains("missing `;` after child node")
    );
    assert!(
        error::<Command>(quote!([::library]; node => { run: handler }))
            .contains("missing `;` after handler")
    );
}

#[test]
fn malformed_rust_expressions_keep_dsl_context() {
    assert!(error::<Command>(quote!([::library]; node. => {})).contains("invalid node expression"));
    assert!(
        error::<Command>(quote!([::library]; node => { run: if ; }))
            .contains("invalid handler expression")
    );
    assert!(
        error::<Commands>(quote!([::library]; if, {})).contains("invalid dispatcher expression")
    );
}

#[test]
fn unknown_modes_are_validated_before_handlers() {
    for tokens in [
        quote!([::library]; d, asnyc {}),
        quote!([::library]; d, asnyc { node => { run sync: handler; }; }),
    ] {
        assert!(error::<Commands>(tokens).contains("unknown command mode `asnyc`"));
    }
    for tokens in [
        quote!([::library]; asnyc; node => {}),
        quote!([::library]; node => { run asnyc: handler; }),
    ] {
        assert!(error::<Command>(tokens).contains("unknown command mode `asnyc`"));
    }
}

#[test]
fn all_input_must_be_consumed() {
    assert!(syn::parse2::<Command>(quote!([::library]; node => {};)).is_err());
    assert!(syn::parse2::<Commands>(quote!([::library]; d, {} unexpected)).is_err());
}

#[test]
fn wide_valid_and_invalid_lists_use_no_sibling_recursion() {
    let nodes: Vec<_> = (0..3000).map(|_| quote!(node => {};)).collect();
    let input: Commands = syn::parse2(quote!([::library]; d, { #(#nodes)* })).unwrap();
    assert_eq!(input.nodes.len(), 3000);
    assert!(!input.expand().is_empty());
    let input: Command = syn::parse2(quote!([::library]; parent => { #(#nodes)* })).unwrap();
    assert_eq!(input.node.children.len(), 3000);
    assert!(!input.node.expand(&input.library, input.mode).is_empty());
    assert!(
        error::<Command>(quote!([::library]; parent => {
            #(#nodes)*
            run: handler;
        }))
        .contains("must precede child nodes")
    );
}

#[test]
fn nested_branches_remain_supported() {
    let mut node = quote!(leaf => { run: handler; });
    for _ in 0..32 {
        node = quote!(parent => { #node; });
    }
    let input: Command = syn::parse2(quote!([::library]; #node)).unwrap();
    let mut node = &input.node;
    for _ in 0..32 {
        assert!(node.handler.is_none());
        node = &node.children[0];
    }
    assert!(node.handler.is_some());
    assert!(!input.node.expand(&input.library, input.mode).is_empty());
}

#[test]
fn computed_receivers_are_evaluated_even_for_empty_lists() {
    let input: Commands = syn::parse2(quote!([::library]; { side_effect(); &mut d }, {})).unwrap();
    let expanded = input.expand().to_string();
    assert_eq!(expanded.matches("side_effect").count(), 1);
    assert!(expanded.contains("& mut"));
    let input: Commands = syn::parse2(quote!([::library]; d, {})).unwrap();
    assert_eq!(input.expand().to_string(), "{ }");
}

#[test]
fn computed_receiver_borrow_keeps_expression_precedence() {
    let input: Commands = syn::parse2(quote!([::library]; first + second, {})).unwrap();
    let block: syn::ExprBlock = syn::parse2(input.expand()).unwrap();
    let syn::Stmt::Local(local) = &block.block.stmts[0] else {
        panic!("expected a dispatcher binding");
    };
    let Expr::Reference(borrow) = &*local.init.as_ref().unwrap().expr else {
        panic!("expected the computed receiver to be borrowed");
    };
    let Expr::Paren(receiver) = &*borrow.expr else {
        panic!("the borrow must apply to the whole expression");
    };
    assert!(matches!(&*receiver.expr, Expr::Binary(_)));
}

#[test]
fn node_and_handler_expressions_are_emitted_once() {
    let input: Command = syn::parse2(quote!([::library]; node_factory() => {
        run: handler_factory();
        child_factory() => {};
    }))
    .unwrap();
    let expanded = input.node.expand(&input.library, input.mode).to_string();
    for name in ["node_factory", "handler_factory", "child_factory"] {
        assert_eq!(expanded.matches(name).count(), 1);
    }
    assert!(expanded.contains(":: library :: brigadier"));
    assert!(!expanded.contains(":: smaragdine :: brigadier"));
}

#[cfg(not(feature = "async"))]
#[test]
fn async_is_rejected_even_without_async_handlers() {
    for tokens in [
        quote!([::library]; d, async {}),
        quote!([::library]; d, async { node => { run sync: handler; }; }),
    ] {
        assert!(error::<Commands>(tokens).contains("requires the async feature"));
    }
    for tokens in [
        quote!([::library]; async; node => {}),
        quote!([::library]; node => { run async: handler; }),
    ] {
        assert!(error::<Command>(tokens).contains("requires the async feature"));
    }
}

#[cfg(feature = "async")]
#[test]
fn local_override_does_not_change_child_defaults() {
    let input: Command = syn::parse2(quote!([::library]; async; parent => {
        run sync: sync_handler;
        child => { run: async_handler; };
    }))
    .unwrap();
    assert_eq!(input.mode, Mode::Async);
    assert_eq!(input.node.handler.as_ref().unwrap().mode, Some(Mode::Sync));
    assert_eq!(input.node.children[0].handler.as_ref().unwrap().mode, None);
    let expanded = input.node.expand(&input.library, input.mode).to_string();
    assert!(expanded.contains(":: executes ("));
    assert!(expanded.contains(":: executes_async ("));
}
