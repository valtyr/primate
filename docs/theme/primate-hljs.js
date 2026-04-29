// Register a `primate` language with highlight.js so ```primate code
// blocks render with proper colors. mdBook ships highlight.js v9-flavored
// API; we use what's available there but feature-detect the v10+ name too.

hljs.registerLanguage("primate", function (hljs) {
  var KEYWORDS = {
    keyword: "namespace enum type use as",
    type:
      "i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 bool string duration bytes regex url " +
      "array optional map tuple",
    literal: "true false none",
  };

  var DOC_COMMENT = {
    className: "doctag",
    begin: "(///|//!).*$",
  };

  var LINE_COMMENT = hljs.COMMENT("//", "$", {
    relevance: 0,
  });

  var ESCAPE = {
    className: "subst",
    begin: "\\\\[nrt0\\\\\"]",
    relevance: 0,
  };

  var STRING = {
    className: "string",
    variants: [
      { begin: '"', end: '"', contains: [ESCAPE] },
      // Raw strings: r"…", r#"…"#, r##"…"##
      { begin: 'r#*"', end: '"#*' },
    ],
  };

  // Numbers, optionally followed by a unit suffix (30s, 100MiB, 1_000ns).
  var NUMBER = {
    className: "number",
    variants: [
      { begin: "\\b0x[0-9A-Fa-f_]+\\b" },
      { begin: "\\b0b[01_]+\\b" },
      { begin: "\\b0o[0-7_]+\\b" },
      {
        begin:
          "\\b\\d[\\d_]*(?:\\.\\d[\\d_]*(?:[eE][+-]?\\d+)?)?(?:[A-Za-z][A-Za-z]*)?\\b",
      },
    ],
  };

  var ATTRIBUTE = {
    className: "meta",
    begin: "@[A-Za-z_][A-Za-z0-9_]*",
    end: "(\\(.*?\\))?",
    relevance: 5,
  };

  // SCREAMING_SNAKE_CASE constants — only color when not a keyword.
  var CONSTANT_NAME = {
    className: "variable.constant",
    begin: "\\b[A-Z][A-Z0-9_]+\\b",
    relevance: 0,
  };

  return {
    name: "primate",
    aliases: ["prim"],
    keywords: KEYWORDS,
    contains: [
      DOC_COMMENT,
      LINE_COMMENT,
      STRING,
      NUMBER,
      ATTRIBUTE,
      CONSTANT_NAME,
    ],
  };
});

// mdBook runs its initial highlight pass on DOMContentLoaded with the
// languages it knows about; primate isn't one of them. Re-highlight after
// our registration. Works for both v9 (highlightBlock) and v10+
// (highlightElement) APIs.
(function () {
  function applyHighlight() {
    var fn =
      typeof hljs.highlightElement === "function"
        ? hljs.highlightElement
        : hljs.highlightBlock;
    var blocks = document.querySelectorAll(
      "pre code.language-primate, pre code.language-prim",
    );
    for (var i = 0; i < blocks.length; i++) {
      var el = blocks[i];
      el.removeAttribute("data-highlighted");
      if (!el.classList.contains("language-primate")) {
        el.classList.add("language-primate");
      }
      fn.call(hljs, el);
    }
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", applyHighlight);
  } else {
    applyHighlight();
  }
})();
