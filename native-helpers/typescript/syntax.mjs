/** 固定 TS 6.0.3 的原生早期诊断分类：来源和排除项见 docs/research/0025-typescript-native-helper-migration.md。
 * 以原生 plainJSErrors 的 binder/grammar 家族为起点，补齐 checker 中的控制流、赋值和类早期错误。
 * 不接受整个 semantic 集合：类型、目标版本、重复普通对象属性等仍允许分析。
 */
const grammarDiagnostics = [
  // 严格模式、绑定与模块（重复导出沿用旧解析器的允许政策）。
  'Invalid_use_of_0_in_strict_mode',
  'with_statements_are_not_allowed_in_strict_mode',
  'delete_cannot_be_called_on_an_identifier_in_strict_mode',
  'Code_contained_in_a_class_is_evaluated_in_JavaScript_s_strict_mode_which_does_not_allow_this_use_of_0_For_more_information_see_https_Colon_Slash_Slashdeveloper_mozilla_org_Slashen_US_Slashdocs_SlashWeb_SlashJavaScript_SlashReference_SlashStrict_mode',
  'Identifier_expected_0_is_a_reserved_word_in_strict_mode_Modules_are_automatically_in_strict_mode',
  'Invalid_use_of_0_Modules_are_automatically_in_strict_mode',
  'A_default_export_must_be_at_the_top_level_of_a_file_or_module_declaration',
  'Identifier_expected_0_is_a_reserved_word_at_the_top_level_of_a_module',
  'This_parameter_is_not_allowed_with_use_strict_directive',
  'use_strict_directive_cannot_be_used_with_non_simple_parameter_list',
  'Identifier_expected_0_is_a_reserved_word_that_cannot_be_used_here',
  'An_import_declaration_can_only_be_used_at_the_top_level_of_a_module',
  'An_export_declaration_can_only_be_used_at_the_top_level_of_a_module',
  // 控制流、函数与异步上下文 18。
  'A_continue_statement_can_only_be_used_within_an_enclosing_iteration_statement',
  'A_break_statement_can_only_be_used_within_an_enclosing_iteration_or_switch_statement',
  'The_left_hand_side_of_a_for_of_statement_may_not_be_async',
  'Jump_target_cannot_cross_function_boundary',
  'A_return_statement_can_only_be_used_within_a_function_body',
  'A_default_clause_cannot_appear_more_than_once_in_a_switch_statement',
  'Duplicate_label_0',
  'A_continue_statement_can_only_jump_to_a_label_of_an_enclosing_iteration_statement',
  'A_break_statement_can_only_jump_to_a_label_of_an_enclosing_statement',
  'A_yield_expression_is_only_allowed_in_a_generator_body',
  'await_expressions_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules',
  'A_label_is_not_allowed_here',
  'yield_expressions_cannot_be_used_in_a_parameter_initializer',
  'await_expressions_cannot_be_used_in_a_parameter_initializer',
  'await_using_statements_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules',
  'await_expression_cannot_be_used_inside_a_class_static_block',
  'for_await_loops_cannot_be_used_inside_a_class_static_block',
  'A_return_statement_cannot_be_used_inside_a_class_static_block',
  // 绑定模式、声明与赋值目标 24。
  'A_rest_parameter_or_binding_pattern_may_not_have_a_trailing_comma',
  'A_rest_parameter_must_be_last_in_a_parameter_list',
  'A_rest_parameter_cannot_have_an_initializer',
  'Only_a_single_variable_declaration_is_allowed_in_a_for_in_statement',
  '_0_declarations_must_be_initialized',
  '_0_declarations_can_only_be_declared_inside_a_block',
  'A_destructuring_declaration_must_have_an_initializer',
  'A_rest_element_cannot_have_an_initializer',
  'Only_a_single_variable_declaration_is_allowed_in_a_for_of_statement',
  'The_variable_declaration_of_a_for_in_statement_cannot_have_an_initializer',
  'The_variable_declaration_of_a_for_of_statement_cannot_have_an_initializer',
  'Catch_clause_variable_cannot_have_an_initializer',
  'The_left_hand_side_of_an_assignment_expression_must_be_a_variable_or_a_property_access',
  'The_left_hand_side_of_a_for_in_statement_must_be_a_variable_or_a_property_access',
  'A_rest_element_must_be_last_in_a_destructuring_pattern',
  'let_is_not_allowed_to_be_used_as_a_name_in_let_or_const_declarations',
  'The_left_hand_side_of_a_for_of_statement_must_be_a_variable_or_a_property_access',
  'The_left_hand_side_of_a_for_in_statement_cannot_be_a_destructuring_pattern',
  'Cannot_redeclare_identifier_0_in_catch_clause',
  'A_rest_element_cannot_contain_a_binding_pattern',
  'A_rest_element_cannot_have_a_property_name',
  'The_left_hand_side_of_an_assignment_expression_may_not_be_an_optional_property_access',
  'The_left_hand_side_of_a_for_in_statement_may_not_be_an_optional_property_access',
  'The_left_hand_side_of_a_for_of_statement_may_not_be_an_optional_property_access',
  // 类、访问器与私有名 18。
  'A_set_accessor_must_have_exactly_one_parameter',
  'A_set_accessor_cannot_have_rest_parameter',
  'A_get_accessor_cannot_have_parameters',
  'Private_field_0_must_be_declared_in_an_enclosing_class',
  'extends_clause_already_seen',
  'Classes_can_only_extend_a_single_class',
  'A_class_declaration_without_the_default_modifier_must_have_a_name',
  'A_class_member_cannot_have_the_0_keyword',
  'Class_constructor_may_not_be_an_accessor',
  'Class_constructor_may_not_be_a_generator',
  'Private_identifiers_are_only_allowed_in_class_bodies_and_may_only_be_used_as_part_of_a_class_member_declaration_property_access_or_on_the_left_hand_side_of_an_in_expression',
  'Super_calls_are_not_permitted_outside_constructors_or_in_nested_functions_inside_constructors',
  'Multiple_constructor_implementations_are_not_allowed',
  'arguments_cannot_be_referenced_in_property_initializers_or_class_static_initialization_blocks',
  'Classes_may_not_have_a_field_named_constructor',
  'The_operand_of_a_delete_operator_cannot_be_a_private_identifier',
  'constructor_is_a_reserved_word',
  'Private_identifiers_are_not_allowed_outside_class_bodies',
  // 其余原生语法 26。
  '_0_expected',
  'Trailing_comma_not_allowed',
  '_0_modifier_must_precede_1_modifier',
  '_0_modifier_already_seen',
  '_0_modifier_cannot_appear_on_class_elements_of_this_kind',
  '_0_modifier_cannot_be_used_here',
  '_0_modifier_cannot_appear_on_a_module_or_namespace_element',
  '_0_modifier_cannot_appear_on_a_constructor_declaration',
  '_0_modifier_cannot_appear_on_a_parameter',
  '_0_list_cannot_be_empty',
  'Variable_declaration_list_cannot_be_empty',
  'An_object_member_cannot_be_declared_optional',
  'A_comma_expression_is_not_allowed_in_a_computed_property_name',
  'Modifiers_cannot_appear_here',
  'An_import_declaration_cannot_have_modifiers',
  'An_export_declaration_cannot_have_modifiers',
  'Line_terminator_not_permitted_before_arrow',
  'A_definite_assignment_assertion_is_not_permitted_in_this_context',
  'Did_you_mean_to_use_a_Colon_An_can_only_follow_a_property_name_when_the_containing_object_literal_is_part_of_a_destructuring_pattern',
  'Argument_of_dynamic_import_cannot_be_spread_element',
  'Tagged_template_expressions_are_not_permitted_in_an_optional_chain',
  'Dynamic_imports_can_only_accept_a_module_specifier_and_an_optional_set_of_attributes_as_arguments',
  'JSX_property_access_expressions_cannot_include_JSX_namespace_names',
  '_0_and_1_operations_cannot_be_mixed_without_parentheses',
  'JSX_attributes_must_only_be_assigned_a_non_empty_expression',
  '_0_is_not_a_valid_meta_property_for_keyword_1_Did_you_mean_2',
];

/** 只对原生诊断所指的私有名消歧：词法声明存在而接收者类型不符，是普通类型错误。
 * 类祖先和两侧成员均由原生符号 API 提供，不创建另一套作用域或绑定索引。
 */
function missingPrivateName(ts, checker, sf, diagnostic) {
  if (![2339, 2304, 18013].includes(diagnostic.code) || diagnostic.start === undefined) return false;
  const name = ts.getTokenAtPosition(sf, diagnostic.start);
  if (!ts.isPrivateIdentifier(name)) return false;
  for (let owner = ts.getContainingClassExcludingClassDecorators(name); owner; owner = ts.getContainingClass(owner)) {
    const symbol = owner.symbol;
    for (const type of [checker.getDeclaredTypeOfSymbol(symbol), checker.getTypeOfSymbolAtLocation(symbol, owner)]) {
      if (checker.getPrivateIdentifierPropertyOfType(type, name.text, name)) return false;
    }
  }
  return true;
}

/** 同一 Program 上同步读取未抑制的原生诊断；所有临时属性连同“是否存在”在 finally 恢复。
 * checker.getDiagnostics 是固定引擎内部入口：绕过注释过滤，但仍复用原 AST、binder 和 checker。
 * 普通 LS 诊断在事务外保留原有 noCheck/指令政策；禁止在此事务中 async、yield 或导航调用。
 */
export function hasSyntaxErrors(ts, program, sf) {
  if (sf.parseDiagnostics.length || program.getSyntacticDiagnostics(sf).length) return true;
  const options = program.getCompilerOptions();
  const originalRedirect = program.isSourceOfProjectReferenceRedirect;
  const changes = [
    [options, 'noCheck', false], [options, 'checkJs', true],
    [options, 'skipLibCheck', false], [options, 'skipDefaultLibCheck', false],
    [sf, 'checkJsDirective', undefined],
    [program, 'isSourceOfProjectReferenceRedirect', file => file === sf.fileName ? false : originalRedirect(file)],
  ];
  const saved = changes.map(([object, key]) => Object.getOwnPropertyDescriptor(object, key));
  try {
    changes.forEach(([object, key, value], index) => Object.defineProperty(object, key, saved[index]
      ? { ...saved[index], value } : { value, writable: true, enumerable: true, configurable: true }));
    const checker = program.getTypeChecker();
    const diagnostics = checker.getDiagnostics(sf);
    // 2300/2451 只接受文件自己的 binder 诊断；checker 的同码跨文件 script 冲突不应拒绝。
    // 导出说明符的 2300 只表示重复导出，不是重复词法绑定；旧入口对此保持宽松。
    if (sf.bindDiagnostics.some(d => d.code === 2451 || d.code === 2300
      && !ts.isExportSpecifier(ts.getTokenAtPosition(sf, d.start).parent))) return true;
    const codes = new Set(grammarDiagnostics.map(name => ts.Diagnostics[name].code));
    return [...sf.bindDiagnostics, ...diagnostics].some(d => codes.has(d.code) || missingPrivateName(ts, checker, sf, d));
  } finally {
    changes.forEach(([object, key], index) => {
      if (saved[index]) Object.defineProperty(object, key, saved[index]);
      else delete object[key];
    });
  }
}
