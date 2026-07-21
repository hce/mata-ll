-- Test suite for the Regex library
import Regex

tryCompile :: String -> (RE -> IO ()) -> IO ()
tryCompile pat f = case compile pat of
    Left err -> putStrLn ("FAIL compile: " <> err)
    Right re -> f re

test_literals :: IO ()
test_literals = do
    tryCompile "abc" (\re -> do
        assert (test re "abc") "literal match"
        assert (test re "xabcx") "literal substring"
        assert (not (test re "axbc")) "literal no match")

test_dot :: IO ()
test_dot =
    tryCompile "a.c" (\re -> do
        assert (test re "abc") "dot match"
        assert (not (test re "ac")) "dot no match")

test_quantifiers :: IO ()
test_quantifiers = do
    tryCompile "ab*c" (\re -> do
        assert (test re "ac") "star zero"
        assert (test re "abbbc") "star many")
    tryCompile "a*" (\re -> do
        assert (matchFull re "") "star empty"
        assert (matchFull re "aaa") "star full")
    tryCompile "ab+c" (\re -> do
        assert (test re "abc") "plus one"
        assert (test re "abbbc") "plus many"
        assert (not (matchFull re "ac")) "plus zero fails")
    tryCompile "ab?c" (\re -> do
        assert (test re "ac") "opt zero"
        assert (test re "abc") "opt one")

test_alternation :: IO ()
test_alternation =
    tryCompile "cat|dog" (\re -> do
        assert (test re "cat") "alt left"
        assert (test re "dog") "alt right"
        assert (not (test re "car")) "alt no match")

test_groups :: IO ()
test_groups =
    tryCompile "(ab)+" (\re ->
        assert (test re "ababab") "group repeat")

test_classes :: IO ()
test_classes = do
    tryCompile "[abc]" (\re -> do
        assert (test re "b") "class match"
        assert (not (test re "d")) "class no match")
    tryCompile "[a-z]" (\re -> do
        assert (test re "m") "range match"
        assert (not (test re "M")) "range no match")
    tryCompile "[^0-9]" (\re -> do
        assert (test re "a") "neg class match"
        assert (not (test re "5")) "neg class no match")

test_anchors :: IO ()
test_anchors = do
    tryCompile "^abc$" (\re ->
        assert (matchFull re "abc") "anchors full")
    tryCompile "^abc" (\re ->
        assert (test re "abcdef") "anchor start")
    tryCompile "def$" (\re ->
        assert (test re "abcdef") "anchor end")

test_escapes :: IO ()
test_escapes = do
    tryCompile "\\d+" (\re ->
        assert (test re "123") "digit class")
    tryCompile "\\w+" (\re ->
        assert (test re "hello_42") "word class")
    tryCompile "\\s" (\re ->
        assert (test re " ") "space class")

test_fullmatch :: IO ()
test_fullmatch =
    tryCompile "[a-z]+" (\re -> do
        assert (matchFull re "hello") "full match yes"
        assert (not (matchFull re "Hello")) "full match no")

test_complex :: IO ()
test_complex = do
    tryCompile "(foo|bar)baz" (\re -> do
        assert (test re "foobaz") "complex alt+seq"
        assert (test re "barbaz") "complex alt+seq 2")
    tryCompile "[a-zA-Z_][a-zA-Z0-9_]*" (\re -> do
        assert (matchFull re "my_var42") "identifier"
        assert (not (matchFull re "42bad")) "identifier fail")

test_errors :: IO ()
test_errors = do
    assert (case compile "[abc" of { Left _ -> True; Right _ -> False }) "unterminated class"
    assert (case compile "(abc" of { Left _ -> True; Right _ -> False }) "unterminated group"

main :: IO ()
main = do
    test_literals
    test_dot
    test_quantifiers
    test_alternation
    test_groups
    test_classes
    test_anchors
    test_escapes
    test_fullmatch
    test_complex
    test_errors
