// Tree-sitter grammar for primate.
//
// This grammar is tuned for syntax highlighting, not for being a complete
// reference parser. It treats whitespace (including newlines) as extras and
// relies on token shape to discriminate declarations. The canonical
// parser/validator is the one shipped in `src/parser/` of the primate repo.

module.exports = grammar({
  name: 'primate',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.doc_comment,
    $.file_doc_comment,
  ],

  word: $ => $.identifier,

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.attribute,
      $.namespace_declaration,
      $.use_declaration,
      $.enum_declaration,
      $.type_alias_declaration,
      $.constant_declaration,
    ),

    line_comment: _ => token(seq('//', /[^/!\n][^\n]*/)),
    doc_comment: _ => token(seq('///', /[^\n]*/)),
    file_doc_comment: _ => token(seq('//!', /[^\n]*/)),

    attribute: $ => seq(
      '@',
      field('name', $.identifier),
      optional(seq('(', commaSep($._attr_arg), ')')),
    ),

    _attr_arg: $ => choice(
      $.identifier,
      $.string_literal,
      $.integer_literal,
      $.boolean_literal,
    ),

    namespace_declaration: $ => seq(
      'namespace',
      field('path', $.qualified_path),
    ),

    // We inline the path rather than reusing `qualified_path` so the leaf in
    // single form (a type name) can be tagged separately from intermediate
    // namespace segments (and so the trailing `::{...}` doesn't compete with
    // `qualified_path`'s greedy `::Ident` repeat).
    use_declaration: $ => seq(
      'use',
      field('first', $.identifier),
      repeat(seq('::', field('segment', $.identifier))),
      '::',
      choice(
        // Single form: leaf is the imported type name.
        field('leaf', $.identifier),
        // Brace form: each item is an imported type name.
        seq(
          '{',
          commaSep1(field('item', $.identifier)),
          optional(','),
          '}',
        ),
      ),
    ),

    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      optional(seq(':', field('backing', $._type_expr))),
      '{',
      commaSep($.enum_variant),
      optional(','),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(seq('=', field('value', $._value))),
    ),

    type_alias_declaration: $ => seq(
      'type',
      field('name', $.identifier),
      '=',
      field('target', $._type_expr),
    ),

    // Constants need higher precedence than the bare-path-as-value to
    // disambiguate `Status DEFAULT = Pending` from a bare-path value
    // followed by another decl.
    constant_declaration: $ => prec(1, seq(
      field('type', $._type_expr),
      field('name', $.identifier),
      '=',
      field('value', $._value),
    )),

    _type_expr: $ => choice(
      $.primitive_type,
      $.container_type_app,
      $.array_type,
      $.optional_type,
      $.qualified_path,
    ),

    array_type: $ => prec(2, seq($._type_expr, '[', ']')),
    optional_type: $ => prec(2, seq($._type_expr, '?')),

    primitive_type: _ => choice(
      'i8', 'i16', 'i32', 'i64',
      'u8', 'u16', 'u32', 'u64',
      'f32', 'f64',
      'bool', 'string', 'duration', 'regex', 'url',
    ),

    container_type: _ => choice('array', 'optional', 'map', 'tuple'),

    container_type_app: $ => seq(
      $.container_type,
      '<',
      commaSep1($._type_expr),
      '>',
    ),

    qualified_path: $ => prec.left(seq(
      $.identifier,
      repeat(seq('::', $.identifier)),
    )),

    _value: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.boolean_literal,
      $.none_literal,
      $.array_literal,
      $.tuple_literal,
      $.map_literal,
      $.qualified_path,
      $.negative_literal,
    ),

    negative_literal: $ => seq('-', choice($.integer_literal, $.float_literal)),

    integer_literal: $ => seq(
      choice(
        /0x[0-9A-Fa-f_]+/,
        /0b[01_]+/,
        /0o[0-7_]+/,
        /\d[\d_]*/,
      ),
      optional($.unit_suffix),
    ),

    float_literal: $ => seq(
      /\d[\d_]*\.\d[\d_]*([eE][+-]?\d+)?/,
      optional($.unit_suffix),
    ),

    // Unit suffix: alphabetic identifier (e.g. `s`, `min`, `MiB`) or a
    // single `%` (percentage on float literals).
    unit_suffix: _ => token.immediate(choice(/[A-Za-z]+/, '%')),

    // String body is a single atomic token. If we let tree-sitter parse it
    // structurally (with `repeat(choice(escape, char))`) then `extras` —
    // including `line_comment` — could fire between inner character tokens,
    // which makes `"foo // bar"` render as a comment past the slashes. By
    // wrapping the whole literal in `token(...)`, no extras can sneak in.
    // We lose the named `escape_sequence` node for highlighting inside the
    // string; that's an acceptable trade for correctness.
    string_literal: _ => token(choice(
      seq(
        '"',
        repeat(choice(
          seq('\\', /[nrt0\\"]/),
          /[^"\\\n]/,
        )),
        '"',
      ),
      // Raw string: `r#*"...."#*` — quotes don't need escaping.
      /r#*"[^"]*"#*/,
    )),

    boolean_literal: _ => choice('true', 'false'),
    none_literal: _ => 'none',

    array_literal: $ => seq('[', commaSep($._value), optional(','), ']'),
    tuple_literal: $ => seq('(', commaSep($._value), optional(','), ')'),
    map_literal: $ => seq(
      '{',
      commaSep(seq(field('key', $._map_key), ':', field('value', $._value))),
      optional(','),
      '}',
    ),
    _map_key: $ => choice($.string_literal, $.identifier, $.integer_literal),

    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
