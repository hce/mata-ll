-- Record construction and update with the opening brace on the line AFTER
-- the constructor/expression (GHC layout: a line indented strictly past the
-- enclosing block column is a continuation). The same-line forms are covered
-- by records.mll / record_update.mll; this case pins the cross-line forms.

data Rec = MkRec { ra :: Integer, rb :: Integer }

mk :: Integer -> Rec
mk n = MkRec
         { ra = n
         , rb = n * 10 }

bump :: Rec -> Rec
bump r = r
           { ra = ra r + 1 }

main :: IO ()
main = do
    let r = mk 5
    assert (ra r == 5) "construction brace on next line"
    assert (rb r == 50) "second field intact"
    let r2 = bump r
    assert (ra r2 == 6) "update brace on next line"
    let r3 = r2 { ra = 7 }
               { rb = 70 }
    assert (ra r3 == 7) "chained update, second brace on next line"
    assert (rb r3 == 70) "chained update value"
    assert (ra (r3
      { ra = 9 }) == 9) "cross-line update inside an argument"
    print (ra r3 + rb r3)
