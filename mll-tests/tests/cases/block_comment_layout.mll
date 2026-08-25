-- A line containing ONLY a block comment is whitespace to layout, like
-- a line comment or a blank line.  Regression: the lexer pushed the
-- line's Indent before seeing `{-`, and the comment then swallowed the
-- rest of the line — the stray Indent pair broke the operator
-- continuation check ("Unexpected token '+' at top level").

total :: Int
total = 1
    {- interleaved block comment -}
    + 2
    {- another one -}
    {- and two on
       adjacent lines -}
    + 30

inDo :: IO ()
inDo = do
    {- comment-only line inside a do block -}
    putStrLn "do ok"

main :: IO ()
main = do
    print total
    inDo
