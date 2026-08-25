-- Haskell 2010 §2.3: a run of two or more dashes opens a comment only
-- when the character after the WHOLE run is not a symbol character —
-- `-->` and `--->` are operator tokens.  Regression: the lexer looked
-- one character past the first two dashes and treated a further dash as
-- "still a comment", so `--->` swallowed the rest of its line.  Plain
-- `--`/`---` comment lines stay comments.

(--->) :: Int -> Int -> Int
a ---> b = a * 100 + b

(-->) :: Int -> Int -> Int
a --> b = a + b

main :: IO ()
main = do
    print (1 ---> 2) -- a trailing comment still works
    print (3 --> 4)
    --- a triple-dash comment line
    print 5
