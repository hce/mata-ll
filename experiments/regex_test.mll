import Regex

tryCompile :: String -> (RE -> IO ()) -> IO ()
tryCompile pat f = case compile pat of
    Left err -> putStrLn ("FAIL: " <> err)
    Right re -> f re

main :: IO ()
main = do
    tryCompile "abc" (\re -> do
        assert (test re "xabcy") "literal: abc in xabcy"
        assert (not (test re "xaby")) "literal: abc not in xaby"
        assert (matchFull re "abc") "literal: full match abc"
        assert (not (matchFull re "abcd")) "literal: no full match abcd")

    tryCompile "a.c" (\re -> do
        assert (test re "abc") "dot: a.c matches abc"
        assert (test re "axc") "dot: a.c matches axc"
        assert (not (test re "ac")) "dot: a.c rejects ac")

    tryCompile "ab*c" (\re -> do
        assert (test re "ac") "star: ab*c matches ac"
        assert (test re "abc") "star: ab*c matches abc"
        assert (test re "abbbbc") "star: ab*c matches abbbbc")

    tryCompile "ab+c" (\re -> do
        assert (not (test re "ac")) "plus: ab+c rejects ac"
        assert (test re "abc") "plus: ab+c matches abc"
        assert (test re "abbc") "plus: ab+c matches abbc")

    tryCompile "colou?r" (\re -> do
        assert (test re "color") "opt: matches color"
        assert (test re "colour") "opt: matches colour")

    tryCompile "cat|dog" (\re -> do
        assert (test re "I have a cat") "alt: finds cat"
        assert (test re "I have a dog") "alt: finds dog"
        assert (not (test re "I have a bird")) "alt: rejects bird")

    tryCompile "(ab)+" (\re -> do
        assert (test re "ababab") "group: (ab)+ matches ababab"
        assert (not (test re "aaa")) "group: (ab)+ rejects aaa")

    tryCompile "^hello" (\re -> do
        assert (test re "hello world") "anchor: ^hello matches start"
        assert (not (test re "say hello")) "anchor: ^hello rejects middle")

    tryCompile "world$" (\re -> do
        assert (test re "hello world") "anchor: world$ matches end"
        assert (not (test re "world!")) "anchor: world$ rejects non-end")

    tryCompile "[aeiou]+" (\re -> do
        assert (test re "hello") "class: vowels in hello"
        assert (not (test re "rhythm")) "class: no vowels in rhythm")

    tryCompile "[0-9]+" (\re -> do
        assert (test re "abc123") "range: digits in abc123"
        assert (not (test re "abcdef")) "range: no digits in abcdef")

    tryCompile "[^0-9]+" (\re -> do
        assert (matchFull re "abc") "nclass: full match abc"
        assert (not (matchFull re "123")) "nclass: no full match 123")

    tryCompile "\\d+\\.\\d+" (\re -> do
        assert (test re "pi is 3.14") "escape: decimal in text"
        assert (not (test re "no numbers")) "escape: no decimal")

    tryCompile "a.*b" (\re -> do
        assert (test re "aXYZb") "greedy: a.*b matches aXYZb"
        assert (test re "ab") "greedy: a.*b matches ab"
        assert (not (test re "aXYZ")) "greedy: a.*b rejects aXYZ")

    tryCompile "" (\re -> assert (test re "anything") "empty: matches anything")

    putStrLn "All regex tests passed!"
