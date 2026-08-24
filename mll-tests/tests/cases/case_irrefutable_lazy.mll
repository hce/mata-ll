-- A `case` whose FIRST pattern is irrefutable (a variable, wildcard,
-- or as-pattern over one) selects that branch without inspecting the
-- scrutinee: GHC binds it unevaluated and later branches are
-- unreachable. Regression: all three case emitters (value, guarded
-- value, action position) forced the scrutinee at entry, so
-- `case undefined of r -> 5` raised where GHC prints 5; and both
-- demand analyses claimed the scrutinee's demand unconditionally, so
-- `lazyArg` below was marked strict in x and callers entry-forced an
-- argument GHC never touches.

val :: Int
val = case undefined of r -> 5

wild :: Int
wild = case undefined of _ -> 7

asWild :: Int
asWild = case undefined of x@_ -> 6

guarded :: Int
guarded = case undefined of r | True -> 8

-- the demand facet: x must not be entry-forced
lazyArg :: Int -> Int
lazyArg x = case x of r -> 40

-- control: demanding the binding demands the scrutinee (still strict)
strictUse :: Int -> Int
strictUse x = case x of r -> r + 1

-- guards force the binding only through use
guardUse :: Int
guardUse = case 41 of
    r | r > 0 -> r + 1
      | otherwise -> 0

-- an irrefutable first clause whose guards all fail falls through to a
-- later refutable clause, which forces per-clause
fallThrough :: Maybe Int -> Int
fallThrough v = case v of
    m | False -> 0
    Just y -> y
    Nothing -> -1

-- refutable first pattern: still forces at entry (control)
refutable :: Maybe Int -> Int
refutable m = case m of
    Just _ -> 1
    Nothing -> 2

main :: IO ()
main = do
    print val
    print wild
    print asWild
    print guarded
    print (lazyArg (error "boom"))
    print (strictUse 42)
    print guardUse
    print (fallThrough (Just 3))
    print (fallThrough Nothing)
    print (refutable (Just 0))
    case undefined of _ -> putStrLn "action ok"
