data Person = Person { personName :: String, personAge :: Int }

main :: IO ()
main = do
    let p = Person "Alice" 30
    assert (personName p == "Alice") "personName accessor"
    assert (personAge p == 30) "personAge accessor"
    assert (p.personName == "Alice") "dot syntax name"
    assert (p.personAge == 30) "dot syntax age"
    -- Named field construction (fields in any order)
    let q = Person { personAge = 25, personName = "Bob" }
    assert (q.personName == "Bob") "named construction name"
    assert (q.personAge == 25) "named construction age"
