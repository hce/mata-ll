-- Force-once at the clause-chain split: a parameter scrutinized only by
-- LATER clauses is rebound (`p = __force(p)`) once the earlier clauses have
-- failed — exactly when the next clause's condition would force it first
-- anyway (codegen later_clause_force_col). GHC clause-order laziness must
-- survive: a matching earlier clause never forces such an argument, and a
-- clause whose left columns block the split keeps its per-use forcing.

data T = Leaf Int | Node T T

-- salsa's lwGo shape: column 0 untouched by clause 0, deep cons pattern in
-- clause 1, irrefutable fall-off. The split rebinds column 0 once.
lw :: [Int] -> Int -> [Int]
lw _ 16 = []
lw (b0 : b1 : b2 : b3 : rest) n = (b0 + b1 + b2 + b3) : lw rest (n + 1)
lw _ _ = []

-- huffman's decode shape, as a where-group (the _warg emitter): column 1
-- (the tree) is untouched by clause 0 and splits after it; column 2 stays
-- per-use forced (clause 3's Node test on column 1 blocks a second split).
decode :: T -> Int -> [Int] -> [Int]
decode tree n bits = go n tree bits
  where
    go 0 _ _ = []
    go k (Leaf s) rest = s : go (k - 1) tree rest
    go k (Node l r) (b : bs) = if b == 0 then go k l bs else go k r bs
    go _ (Node _ _) [] = [99]

-- Ineligible split: clause 1's literal in column 0 means failing PAST it
-- must not force column 1 (GHC reaches clause 2 with the error untouched).
sel :: Int -> Maybe Int -> Int
sel 0 _ = 1
sel 1 (Just y) = y
sel _ _ = 99

main :: IO ()
main = do
    -- A matching earlier clause never forces the later-scrutinized column.
    assert (lw (error "lw list unforced") 16 == []) "lw _ 16 lazy in list"
    assert (decode (error "tree unforced") 0 (error "bits unforced") == [])
        "decode 0 lazy in tree and bits"
    assert (sel 0 (error "sel unforced") == 1) "sel 0 lazy"
    -- Skipping a blocked clause must not force its right column either.
    assert (sel 2 (error "sel fall-through unforced") == 99) "sel _ _ lazy"
    -- The split paths still compute correctly (rebound param read bare).
    assert (lw [1, 2, 3, 4, 5, 6, 7, 8] 0 == [10, 26]) "lw sums quads"
    assert (lw [1, 2, 3] 5 == []) "lw short list falls through"
    let tree = Node (Leaf 1) (Node (Leaf 2) (Leaf 3))
    assert (decode tree 3 [0, 1, 0, 1, 1] == [1, 2, 3]) "decode walks tree"
    assert (decode tree 2 [1, 1] == [3, 99]) "decode ran out of bits"
    putStrLn "later-clause force-once ok"
-- expect: later-clause force-once ok
