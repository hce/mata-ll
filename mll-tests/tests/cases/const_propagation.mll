-- Cross-binding constant folding: literal CAFs propagate into use
-- sites, saturated calls to total-arithmetic functions on literal
-- arguments beta-reduce, and chains converge over folding rounds
-- (fold.rs fixpoint). All of it must be OBSERVATIONALLY INVISIBLE —
-- these are the values the unfolded program computes; GHC is the
-- oracle. The shadowing and decline cases below pin the boundaries:
-- local binders mask top-level names, and anything the folds decline
-- (a trapping divisor) keeps its runtime (and lazy) behavior.

abc :: Int
abc = 17

def :: Int -> Int
def x = x + 1

ghi :: Int
ghi = abc + def 5

-- converges one binding per round
chainA :: Int
chainA = 1

chainB :: Int
chainB = chainA + 1

chainC :: Int
chainC = chainB + chainB

-- string CAF propagation + <> fold
greet :: String
greet = "hello"

greeting :: String
greeting = greet <> " world"

-- Bool CAF propagation + if-fold
flag :: Bool
flag = True

picked :: Int
picked = if flag then ghi else 0

-- a parameter shadows the CAF: 17 must NOT substitute here
shadowed :: Int -> Int
shadowed abc = abc + 1

-- a where-bind shadows the arithmetic candidate: the LOCAL def runs
shadowedFn :: Int
shadowedFn = def 10
  where def y = y * 2

-- a candidate passed first-class is an ordinary function value
applied :: Int
applied = go def
  where go f = f 6

-- declined fold, preserved laziness: a literal zero divisor is left for
-- the runtime, and the CAF must raise only when demanded — never here
troubleDiv :: Int
troubleDiv = 1 `div` 0

useTrouble :: Int -> Int
useTrouble x = if x > 0 then x else troubleDiv

-- Number folding
half :: Number
half = 1.0 / 2.0

main :: IO ()
main = do
    print ghi
    print chainC
    putStrLn greeting
    print picked
    print (shadowed 1)
    print shadowedFn
    print applied
    print (useTrouble 5)
    print (half + 0.25)
