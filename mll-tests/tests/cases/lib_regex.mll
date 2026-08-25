import Regex

tryCompile :: String -> (RE -> IO ()) -> IO ()
tryCompile pat f = case compile pat of
    Left err -> putStrLn ("FAIL: compile error for /" <> pat <> "/: " <> err)
    Right re -> f re

isLeft :: Either String RE -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

main :: IO ()
main = do
    -- Literals
    tryCompile "abc" (\re -> do
        assert (test re "xabcy") "lit: abc in xabcy"
        assert (not (test re "xaby")) "lit: abc not in xaby"
        assert (matchFull re "abc") "lit: full match abc"
        assert (not (matchFull re "abcd")) "lit: no full abcd")

    -- Dot
    tryCompile "a.c" (\re -> do
        assert (test re "abc") "dot: abc"
        assert (test re "axc") "dot: axc"
        assert (not (test re "ac")) "dot: rejects ac"
        assert (not (test re "a\nc")) "dot: rejects newline")

    -- Star
    tryCompile "ab*c" (\re -> do
        assert (test re "ac") "star: ac"
        assert (test re "abc") "star: abc"
        assert (test re "abbbbc") "star: abbbbc")

    -- Plus
    tryCompile "ab+c" (\re -> do
        assert (not (test re "ac")) "plus: rejects ac"
        assert (test re "abc") "plus: abc"
        assert (test re "abbc") "plus: abbc")

    -- Optional
    tryCompile "colou?r" (\re -> do
        assert (test re "color") "opt: color"
        assert (test re "colour") "opt: colour"
        assert (not (test re "colouur")) "opt: rejects colouur")

    -- Alternation
    tryCompile "cat|dog" (\re -> do
        assert (test re "I have a cat") "alt: cat"
        assert (test re "I have a dog") "alt: dog"
        assert (not (test re "I have a bird")) "alt: no bird")

    -- Groups
    tryCompile "(ab)+" (\re -> do
        assert (test re "ababab") "group: ababab"
        assert (not (test re "aaa")) "group: rejects aaa")

    -- Nested groups
    tryCompile "((a|b)c)+" (\re -> do
        assert (test re "acbc") "nested group: acbc"
        assert (not (test re "cc")) "nested group: rejects cc")

    -- Anchors
    tryCompile "^hello" (\re -> do
        assert (test re "hello world") "anchor: ^hello start"
        assert (not (test re "say hello")) "anchor: ^hello rejects mid")

    tryCompile "world$" (\re -> do
        assert (test re "hello world") "anchor: world$ end"
        assert (not (test re "world!")) "anchor: world$ rejects")

    tryCompile "^exact$" (\re -> do
        assert (test re "exact") "anchor: ^exact$"
        assert (not (test re "not exact")) "anchor: ^exact$ rejects prefix"
        assert (not (test re "exact!")) "anchor: ^exact$ rejects suffix")

    -- Character classes
    tryCompile "[aeiou]+" (\re -> do
        assert (test re "hello") "class: vowels in hello"
        assert (not (test re "rhythm")) "class: no vowels in rhythm")

    tryCompile "[0-9]+" (\re -> do
        assert (test re "abc123") "range: digits"
        assert (not (test re "abcdef")) "range: no digits")

    -- Negated character class
    tryCompile "[^0-9]+" (\re -> do
        assert (matchFull re "abc") "nclass: abc"
        assert (not (matchFull re "123")) "nclass: rejects 123")

    -- A negated class matches newline (only '.' excludes it — PCRE/POSIX/
    -- JS/Python all match "\n" against [^a]); \D likewise; \S still
    -- rejects it through its own items (newline IS a space).
    tryCompile "[^a]" (\re -> do
        assert (matchFull re "\n") "nclass: matches newline"
        assert (not (matchFull re "a")) "nclass: still rejects the listed byte")
    tryCompile "\\D" (\re ->
        assert (matchFull re "\n") "backslash-D: matches newline")
    tryCompile "\\S" (\re ->
        assert (not (matchFull re "\n")) "backslash-S: rejects newline via its items")

    -- Escape sequences
    tryCompile "\\d+" (\re -> do
        assert (test re "abc123") "\\d: digits"
        assert (not (test re "abcdef")) "\\d: no digits"
        assert (matchFull re "42") "\\d: full match 42")

    tryCompile "\\w+" (\re -> do
        assert (test re "hello_world") "\\w: word chars"
        assert (matchFull re "abc123_XYZ") "\\w: full word")

    tryCompile "\\s+" (\re -> do
        assert (test re "hello world") "\\s: space"
        assert (test re "tab\there") "\\s: tab"
        assert (not (test re "nospaces")) "\\s: no spaces")

    -- Negated escape classes
    tryCompile "\\D+" (\re -> do
        assert (test re "abc") "\\D: non-digits"
        assert (not (matchFull re "123")) "\\D: rejects all digits")

    tryCompile "\\W+" (\re -> do
        assert (test re "hello world") "\\W: non-word (space)"
        assert (not (matchFull re "abc")) "\\W: rejects word-only")

    -- Greedy matching
    tryCompile "a.*b" (\re -> do
        assert (test re "aXYZb") "greedy: aXYZb"
        assert (test re "ab") "greedy: ab"
        assert (not (test re "aXYZ")) "greedy: rejects no b"
        assert (test re "aXbYb") "greedy: multiple b's")

    -- Empty pattern
    tryCompile "" (\re -> do
        assert (test re "anything") "empty: matches anything"
        assert (test re "") "empty: matches empty")

    -- findStr
    tryCompile "\\d+" (\re -> do
        let r1 = findStr re "abc123def"
        assert (r1 == Just "123") "findStr: 123"
        let r2 = findStr re "no digits"
        assert (r2 == Nothing) "findStr: no match")

    -- Escaped special characters
    tryCompile "\\." (\re -> do
        assert (test re "a.b") "escaped dot: matches literal"
        assert (not (matchFull re "x")) "escaped dot: rejects non-dot")

    tryCompile "\\(" (\re -> do
        assert (test re "(hello)") "escaped paren: matches literal")

    -- Complex patterns
    tryCompile "\\d+\\.\\d+" (\re -> do
        assert (test re "pi is 3.14") "complex: decimal"
        assert (not (test re "no decimal")) "complex: no decimal")

    tryCompile "[a-zA-Z_][a-zA-Z0-9_]*" (\re -> do
        assert (matchFull re "my_var2") "ident: my_var2"
        assert (not (matchFull re "2bad")) "ident: rejects leading digit")

    -- Compile errors
    assert (isLeft (compile "(unclosed")) "error: unclosed paren"

    -- F14: a quantifier with nothing to repeat is an error (it used to
    -- compile as a literal '*'/'+'/'?')
    assert (isLeft (compile "*a")) "error: leading * has nothing to repeat"
    assert (isLeft (compile "+a")) "error: leading + has nothing to repeat"
    assert (isLeft (compile "?a")) "error: leading ? has nothing to repeat"
    assert (isLeft (compile "a|*b")) "error: * after | has nothing to repeat"

    -- F14: unknown ALPHANUMERIC escapes are errors (\b silently matched a
    -- literal 'b'); escaped punctuation stays an identity escape
    assert (isLeft (compile "\\b")) "error: unsupported escape \\b"
    assert (isLeft (compile "\\q")) "error: unsupported escape \\q"
    assert (isLeft (compile "[\\b]")) "error: unsupported escape in class"
    tryCompile "a\\.b" (\re -> do
        assert (test re "xa.by") "identity escape: dot literal matches"
        assert (not (test re "xaXby")) "identity escape: dot literal rejects X")
    tryCompile "\\(x\\)" (\re ->
        assert (test re "f(x)") "identity escape: parens")
    assert (isLeft (compile "[unclosed")) "error: unclosed class"
