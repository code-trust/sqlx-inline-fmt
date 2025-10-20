(
  (macro_invocation
    macro: [
      (identifier) @name
      (scoped_identifier) @name
    ]
    (token_tree
      [
        (string_literal)     @lit
        (raw_string_literal) @raw
      ]
    )
  ) @inv
  (#match? @name "(^|.*::)query(_as|_scalar)?(_unchecked)?$")
)
