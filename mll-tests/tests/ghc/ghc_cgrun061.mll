-- GHC cgrun061: Red-black tree invariant checking

data RBColor = RBRed | RBBlack
    deriving (Show, Eq)

data RBTree = RBLeaf | RBNode RBColor RBTree Int RBTree
    deriving (Show, Eq)

-- Check BST property
isBST :: RBTree -> Bool
isBST t = isBSTHelper t Nothing Nothing

isBSTHelper :: RBTree -> Maybe Int -> Maybe Int -> Bool
isBSTHelper RBLeaf _ _ = True
isBSTHelper (RBNode _ l x r) lo hi =
    loOk lo x && hiOk x hi && isBSTHelper l lo (Just x) && isBSTHelper r (Just x) hi
  where
    loOk Nothing _    = True
    loOk (Just lo_) x = lo_ < x
    hiOk _ Nothing    = True
    hiOk x (Just hi_) = x < hi_

-- Black height: -1 if violated
blackHeight :: RBTree -> Int
blackHeight RBLeaf = 1
blackHeight (RBNode c l _ r) =
    let lh = blackHeight l
        rh = blackHeight r
    in if lh == -1 || rh == -1 || lh /= rh then -1
       else lh + (if c == RBBlack then 1 else 0)

-- No red node has a red child
getColor :: RBTree -> RBColor
getColor RBLeaf = RBBlack
getColor (RBNode c _ _ _) = c

isRed :: RBTree -> Bool
isRed t = getColor t == RBRed

noRedRed :: RBTree -> Bool
noRedRed RBLeaf = True
noRedRed (RBNode c l _ r) =
    if c == RBRed
    then not (isRed l) && not (isRed r) && noRedRed l && noRedRed r
    else noRedRed l && noRedRed r

isValidRB :: RBTree -> Bool
isValidRB t = isBST t && noRedRed t && blackHeight t /= -1

main :: IO ()
main = do
    let t = RBNode RBBlack
                (RBNode RBRed (RBNode RBBlack RBLeaf 1 RBLeaf) 2 (RBNode RBBlack RBLeaf 3 RBLeaf))
                4
                (RBNode RBRed (RBNode RBBlack RBLeaf 5 RBLeaf) 6 (RBNode RBBlack RBLeaf 7 RBLeaf))
    assert (isValidRB t) "valid RB tree"
    assert (blackHeight t == 3) "black height 3"

    -- Red-red violation
    let bad = RBNode RBBlack (RBNode RBRed (RBNode RBRed RBLeaf 1 RBLeaf) 2 RBLeaf) 3 RBLeaf
    assert (not (noRedRed bad)) "red-red violation"

    -- Unequal black heights
    let unbal = RBNode RBBlack (RBNode RBBlack RBLeaf 1 RBLeaf) 2 RBLeaf
    assert (blackHeight unbal == -1) "unequal heights"

    assert (isValidRB RBLeaf) "leaf is valid"
    putStrLn "ok"
