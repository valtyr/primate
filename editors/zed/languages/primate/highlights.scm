; Tree-sitter highlight queries for primate.
; These match the node names produced by tree-sitter-primate (see ../../tree-sitter-primate/).

(line_comment) @comment
(doc_comment) @comment.doc
(file_doc_comment) @comment.doc

["namespace" "enum" "type" "use"] @keyword

(use_declaration first: (identifier) @namespace)
(use_declaration segment: (identifier) @namespace)
(use_declaration leaf: (identifier) @type)
(use_declaration item: (identifier) @type)
["true" "false"] @boolean
(none_literal) @constant.builtin

[(primitive_type) (container_type)] @type.builtin

(enum_declaration name: (_) @type.definition)
(type_alias_declaration name: (_) @type.definition)
(enum_variant name: (_) @variant)
(qualified_path
  (identifier) @namespace
  "::" @punctuation.delimiter)

(constant_declaration name: (_) @constant)
(constant_declaration type: (_) @type)

; A qualified path used as a constant's value is a variant reference
; (`Info` or `LogLevel::Info`). Tag every identifier in the path as a
; variant — this is more specific than the generic `qualified_path`
; rule above, so editors prefer it for value-position paths.
(constant_declaration
  value: (qualified_path
    (identifier) @variant))

(integer_literal) @number
(float_literal) @number
(unit_suffix) @keyword.unit
(string_literal) @string

(attribute name: (_) @function.attribute)
"@" @punctuation.special

["{" "}" "[" "]" "(" ")" "<" ">"] @punctuation.bracket
["," ":" "::" "=" "?"] @punctuation.delimiter
