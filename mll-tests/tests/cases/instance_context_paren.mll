-- Regression: a parenthesized instance context, `instance (Show a) => …`,
-- used to fail to parse at all ("Expected type/constructor name, found
-- LeftParen"). It must parse exactly like the bare form.
data Wrap a = Wrap a

instance (Show a) => Show (Wrap a) where
    show (Wrap x) = "Wrap " <> show x

main :: IO ()
main = do
    assert (show (Wrap 7) == "Wrap 7") "parenthesized context at Wrap Int"
    assert (show (Wrap (Wrap "x")) == "Wrap Wrap \"x\"") "parenthesized context, nested"
