-- W67 (O12, M14): a `.` chain of 120 functions, twice the length that exceeded
-- the host parser's nesting (55). Composition is associative and lazy in
-- both operands; a chain is cut into let-bound compositions, each a
-- closure that evaluates nothing when built, and `const 7 . …` applied to
-- `undefined` never runs the inner chain, exactly as GHC. Pinned against
-- GHC: the composed value, the chain under `map`, a chain of partial
-- applications and the lazy prefix.

step :: Int -> Int
step = succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ

lazyPrefix :: Int -> Int
lazyPrefix = const 7 . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ . succ

addAll :: Int -> Int
addAll = (+ 0) . (+ 1) . (+ 2) . (+ 3) . (+ 4) . (+ 5) . (+ 6) . (+ 7) . (+ 8) . (+ 9) . (+ 10) . (+ 11) . (+ 12) . (+ 13) . (+ 14) . (+ 15) . (+ 16) . (+ 17) . (+ 18) . (+ 19) . (+ 20) . (+ 21) . (+ 22) . (+ 23) . (+ 24) . (+ 25) . (+ 26) . (+ 27) . (+ 28) . (+ 29) . (+ 30) . (+ 31) . (+ 32) . (+ 33) . (+ 34) . (+ 35) . (+ 36) . (+ 37) . (+ 38) . (+ 39) . (+ 40) . (+ 41) . (+ 42) . (+ 43) . (+ 44) . (+ 45) . (+ 46) . (+ 47) . (+ 48) . (+ 49) . (+ 50) . (+ 51) . (+ 52) . (+ 53) . (+ 54) . (+ 55) . (+ 56) . (+ 57) . (+ 58) . (+ 59) . (+ 60) . (+ 61) . (+ 62) . (+ 63) . (+ 64) . (+ 65) . (+ 66) . (+ 67) . (+ 68) . (+ 69) . (+ 70) . (+ 71) . (+ 72) . (+ 73) . (+ 74) . (+ 75) . (+ 76) . (+ 77) . (+ 78) . (+ 79) . (+ 80) . (+ 81) . (+ 82) . (+ 83) . (+ 84) . (+ 85) . (+ 86) . (+ 87) . (+ 88) . (+ 89) . (+ 90) . (+ 91) . (+ 92) . (+ 93) . (+ 94) . (+ 95) . (+ 96) . (+ 97) . (+ 98) . (+ 99) . (+ 100) . (+ 101) . (+ 102) . (+ 103) . (+ 104) . (+ 105) . (+ 106) . (+ 107) . (+ 108) . (+ 109) . (+ 110) . (+ 111) . (+ 112) . (+ 113) . (+ 114) . (+ 115) . (+ 116) . (+ 117) . (+ 118) . (+ 119)

onLists :: [Int] -> [Int]
onLists = map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1) . map (+ 1)

main :: IO ()
main = do
    print (step 0, step 5, map step [1, 2, 3])
    print (lazyPrefix undefined)
    print (addAll 0, addAll 1000)
    print (take 3 (onLists [1 ..]))
    assert (step 0 == 120) "compose chain"
