-- Always-cheap parameters vs partial application: a partial
-- application's closure forwards its remaining parameters RAW — a
-- delivery the call-site scan cannot see. With one full call passing
-- cheap arguments (the tuple keeps the call out of the constant
-- folder's splice, which would hide the site), the uncovered second
-- position was granted always-cheap and `pick` skipped its entry
-- force: the thunk the partial application delivered was then compared
-- as a VALUE, and the native == against a thunk table returned false —
-- a wrong answer, not a crash. Every position a call site does not
-- cover with its own spine must be judged thunked.

module Main where

pick :: (String, String) -> String -> String
pick pa pb = if pb == "q" then "YES" else grow "NO"

useIt :: (String -> String) -> String
useIt kf = kf (grow "q")

grow :: String -> String
grow s = s <> ""

main :: IO ()
main = do
    putStrLn (pick ("a", "b") "q")
    putStrLn (useIt (pick ("a", "b")))

-- expect: YES
-- expect: YES
