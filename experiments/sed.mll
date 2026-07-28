-- sed.mll — a stream editor written in mata-ll, GNU-sed-compatible on its
-- supported feature set.
--
-- WHAT IT IS
--   Reads stdin line by line and applies a sed script given via -e/-f or as
--   the first program argument. Input is read lazily (no lookahead unless the
--   script uses `$` addressing), so interactive scripts — sed games included —
--   respond move by move.
--
-- SUPPORTED
--   * CLI                -n, -e script, -f script-file, combined flags (-nf),
--                        bare script operand, `--`; stdin is the only input
--   * #n                 first line of the script enables -n
--   * comments           # to end of line
--   * addresses          N (line number), $ (last line), /regex/, \cREc,
--                        empty // (reuse the most recently applied regex),
--                        `!` negation
--   * blocks             { ... }, nested
--   * commands           s y p d q Q h H g G x i a b t T : { } = z
--   * s flags            g, p, N (replace Nth occurrence; Ng = Nth onward)
--   * replacements       & and \0 (whole match), \1..\9, \n \t \r \a \f \v,
--                        backslash-newline, \& \\ \x literals
--   * y                  transliteration with \n \t \\ \<delim> escapes
--   * i/a text           classic `i\` + backslash-continued lines and the
--                        GNU one-line `i text` form; \t etc. processed
--   * q/Q exit codes     `q5` exits with status 5
--
-- REGEX FLAVOUR (built-in engine, POSIX BRE with the GNU escapes sed uses)
--   .  matches ANY byte, newline included (pattern-space semantics)
--   ^ $        anchor at pattern-space start/end only (special only in
--              anchor position, literal elsewhere — BRE rules)
--   * \+ \? \{m\} \{m,\} \{m,n\}   repetition
--   \( \)      capture groups, \1..\9 backreferences (also in the pattern)
--   [...] [^...]  classes with ranges; newline matchable via [^x]
--   \n \t \r \a \f \v \w \W \s \S  escapes; \x is the literal x otherwise
--   Matching is leftmost, greedy backtracking. On the BRE subset without
--   alternation this coincides with GNU sed's leftmost-longest semantics.
--
-- NOT SUPPORTED (parse errors name the gap explicitly)
--   * two-address ranges addr1,addr2
--   * commands n N P D c w r R W l e F v
--   * s flags w e m M i I; alternation \|; POSIX [:classes:]
--
-- KNOWN DEVIATION FROM GNU sed
--   Every output line ends in a newline. Real sed preserves the ABSENCE of a
--   trailing newline on the last input line; we cannot, because getLine
--   strips the newline and does not report whether one was present.
--
-- EXAMPLE INVOCATIONS (run in-tree; the mll binary auto-loads lib/)
--   printf 'foo\nbar\n' | target/release/mll -r experiments/sed.mll s/foo/baz/
--   printf 'a\nb\nc\n'  | target/release/mll -r experiments/sed.mll -n /b/p
--   target/release/mll -r experiments/sed.mll -n -f game.sed   # interactive
--
-- Everything after the script filename becomes getArgs, so mll's own flags
-- (like -r) go BEFORE the filename; sed's own flags go after it.

import LIO (fOpen, fRead, fClose, flushStdout)
import LString (strByte, strLen, strSub, strChar)

-- Byte codes used throughout (mata-ll has no char literals):
--   9 tab   10 \n   13 \r   32 space   33 !   35 #   36 $   38 &   40 (
--   41 )    42 *    44 ,    45 -       46 .   47 /   58 :   59 ;   61 =
--   91 [    92 \    93 ]    94 ^       95 _   123 {  125 }
--   48..57 '0'..'9'   65..90 'A'..'Z'   97..122 'a'..'z'

-- === Regex: syntax tree ====================================================

data CItem = CRange Int Int | CByte Int

data RNode = RLit Int    -- literal byte
  | RAny                  -- . — any byte, including newline
  | RClass Bool [CItem]   -- [items] / [^items]; negated classes match \n too
  | RStart                -- ^
  | REnd                  -- $
  | RGroup Int [RNode]    -- \( ... \), numbered by opening order
  | RStar RNode           -- e*
  | RRep RNode Int Int    -- e\{m,n\}; n = -1 means unbounded
  | RBack Int             -- \1..\9 backreference inside the pattern

data RE = RE [RNode] Int  -- nodes, number of capture groups

-- === Regex: parser (POSIX BRE + the GNU escapes sed scripts rely on) =======

compileRe :: String -> Either String RE
compileRe p =
  case reSeq p 1 (strLen p) 0 False of
    Left err -> Left err
    Right (ns, g, i) ->
      if i > strLen p
      then Right (RE ns g)
      else Left "unmatched \\) in regex"

-- Parse a node sequence until end of pattern or a closing \) (whose position
-- is returned unconsumed for the RGroup caller). Third result is the stop
-- index; the Int in the middle is the running capture-group count.
reSeq :: String -> Int -> Int -> Int -> Bool -> Either String ([RNode], Int, Int)
reSeq p i len g inGroup = reSeqGo p i len g inGroup True []

reSeqGo :: String -> Int -> Int -> Int -> Bool -> Bool -> [RNode] -> Either String ([RNode], Int, Int)
reSeqGo p i len g inGroup atStart acc =
  if i > len
  then if inGroup
       then Left "unterminated \\( group in regex"
       else Right (reverse acc, g, i)
  else if strByte p i == 92 && i + 1 <= len && strByte p (i + 1) == 41  -- \)
  then if inGroup
       then Right (reverse acc, g, i)
       else Left "unmatched \\) in regex"
  else case reAtom p i len g atStart of
         Left err -> Left err
         Right (node, g2, j) ->
           case rePostfix p j len node of
             Left err -> Left err
             Right (node2, k) -> reSeqGo p k len g2 inGroup False (node2 : acc)

reAtom :: String -> Int -> Int -> Int -> Bool -> Either String (RNode, Int, Int)
reAtom p i len g atStart =
  let b = strByte p i
  in if b == 46 then Right (RAny, g, i + 1)                               -- .
     else if b == 94                                                      -- ^
     then if atStart then Right (RStart, g, i + 1) else Right (RLit 94, g, i + 1)
     else if b == 36                                                      -- $
     then if reDollarSpecial p i len
          then Right (REnd, g, i + 1)
          else Right (RLit 36, g, i + 1)
     else if b == 91 then reClass p (i + 1) len g                         -- [
     else if b == 92 then reEscape p i len g                              -- \x
     else Right (RLit b, g, i + 1)   -- also covers a leading literal `*`

-- In BRE, `$` is an anchor only at the end of the pattern or of a \( \)
-- subexpression; elsewhere it is a literal dollar sign.
reDollarSpecial :: String -> Int -> Int -> Bool
reDollarSpecial p i len =
  i == len
  || (i + 2 <= len && strByte p (i + 1) == 92 && strByte p (i + 2) == 41)

reEscape :: String -> Int -> Int -> Int -> Either String (RNode, Int, Int)
reEscape p i len g =
  if i + 1 > len
  then Left "trailing backslash in regex"
  else
    let b2 = strByte p (i + 1)
    in if b2 == 40                                                        -- \(
       then let gi = g + 1
            in case reSeq p (i + 2) len gi True of
                 Left err -> Left err
                 Right (ns, g2, j) -> Right (RGroup gi ns, g2, j + 2)
       else if b2 >= 49 && b2 <= 57 then Right (RBack (b2 - 48), g, i + 2)
       else if b2 == 110 then Right (RLit 10, g, i + 2)                   -- \n
       else if b2 == 116 then Right (RLit 9, g, i + 2)                    -- \t
       else if b2 == 114 then Right (RLit 13, g, i + 2)                   -- \r
       else if b2 == 97 then Right (RLit 7, g, i + 2)                     -- \a
       else if b2 == 102 then Right (RLit 12, g, i + 2)                   -- \f
       else if b2 == 118 then Right (RLit 11, g, i + 2)                   -- \v
       else if b2 == 119 then Right (RClass False wordItems, g, i + 2)    -- \w
       else if b2 == 87 then Right (RClass True wordItems, g, i + 2)      -- \W
       else if b2 == 115 then Right (RClass False spaceItems, g, i + 2)   -- \s
       else if b2 == 83 then Right (RClass True spaceItems, g, i + 2)     -- \S
       else if b2 == 124 then Left "alternation \\| is not supported (POSIX BRE has none)"
       else if b2 == 123 then Left "\\{ repetition with nothing to repeat"
       else if b2 == 43 then Left "\\+ repetition with nothing to repeat"
       else if b2 == 63 then Left "\\? repetition with nothing to repeat"
       else Right (RLit b2, g, i + 2)             -- \. \* \[ \] \$ \^ \\ \/ …

wordItems :: [CItem]
wordItems = [CRange 97 122, CRange 65 90, CRange 48 57, CByte 95]

spaceItems :: [CItem]
spaceItems = [CByte 32, CByte 9, CByte 10, CByte 13, CByte 11, CByte 12]

-- i points just past the opening `[`.
reClass :: String -> Int -> Int -> Int -> Either String (RNode, Int, Int)
reClass p i len g =
  if i > len
  then Left "unterminated [ class in regex"
  else
    let neg = strByte p i == 94
        i0 = if neg then i + 1 else i
    in case reClassItems p i0 len True [] of
         Left err -> Left err
         Right (items, j) -> Right (RClass neg items, g, j)

-- A `]` in first position is a literal member; `a-z` is a range; a `-` at
-- either end is literal. Backslash is NOT special inside a class (POSIX).
reClassItems :: String -> Int -> Int -> Bool -> [CItem] -> Either String ([CItem], Int)
reClassItems p i len first acc =
  if i > len
  then Left "unterminated [ class in regex"
  else
    let b = strByte p i
    in if b == 93 && not first
       then Right (reverse acc, i + 1)
       else if b == 91 && i + 1 <= len && isClassOpener (strByte p (i + 1))
       then Left "POSIX [:name:], [.sym.], [=eq=] class syntax is not supported"
       else if i + 2 <= len && strByte p (i + 1) == 45 && strByte p (i + 2) /= 93
       then reClassItems p (i + 3) len False (CRange b (strByte p (i + 2)) : acc)
       else reClassItems p (i + 1) len False (CByte b : acc)

isClassOpener :: Int -> Bool
isClassOpener b = b == 58 || b == 46 || b == 61     -- :  .  =

-- Postfix repetition operators; loops so `a*\{2\}` composes. `^` takes no
-- postfix (a `*` right after it is a literal, per BRE).
rePostfix :: String -> Int -> Int -> RNode -> Either String (RNode, Int)
rePostfix p i len node =
  case node of
    RStart -> Right (node, i)
    _ ->
      if i <= len && strByte p i == 42                                    -- *
      then rePostfix p (i + 1) len (RStar node)
      else if i + 1 <= len && strByte p i == 92 && strByte p (i + 1) == 43   -- \+
      then rePostfix p (i + 2) len (RRep node 1 (0 - 1))
      else if i + 1 <= len && strByte p i == 92 && strByte p (i + 1) == 63   -- \?
      then rePostfix p (i + 2) len (RRep node 0 1)
      else if i + 1 <= len && strByte p i == 92 && strByte p (i + 1) == 123  -- \{
      then case reInterval p (i + 2) len of
             Left err -> Left err
             Right (lo, hi, j) -> rePostfix p j len (RRep node lo hi)
      else Right (node, i)

-- \{m\}  \{m,\}  \{m,n\} — i points past the `\{`.
reInterval :: String -> Int -> Int -> Either String (Int, Int, Int)
reInterval p i len =
  case readNum p i len 0 of
    (lo, j) ->
      if j <= len && strByte p j == 44                                    -- ,
      then if j + 1 <= len && isDigit (strByte p (j + 1))
           then case readNum p (j + 1) len 0 of
                  (hi, k) -> reIntervalClose p k len lo hi
           else reIntervalClose p (j + 1) len lo (0 - 1)
      else reIntervalClose p j len lo lo

reIntervalClose :: String -> Int -> Int -> Int -> Int -> Either String (Int, Int, Int)
reIntervalClose p i len lo hi =
  if i + 1 <= len && strByte p i == 92 && strByte p (i + 1) == 125        -- \}
  then if hi >= 0 && hi < lo
       then Left "bad interval: \\{m,n\\} needs m <= n"
       else Right (lo, hi, i + 2)
  else Left "unterminated \\{m,n\\} interval in regex"

isDigit :: Int -> Bool
isDigit b = b >= 48 && b <= 57

readNum :: String -> Int -> Int -> Int -> (Int, Int)
readNum s i len acc =
  if i > len
  then (acc, i)
  else
    let b = strByte s i
    in if isDigit b
       then readNum s (i + 1) len (acc * 10 + (b - 48))
       else (acc, i)

-- === Regex: backtracking matcher with captures =============================
-- Captures are an assoc list group -> (start, end-exclusive); newest entry
-- first, so backtracking rolls back naturally as continuations unwind.

data MRes = MRes Int [(Int, (Int, Int))]      -- end (exclusive), captures
data Found = Found Int Int [(Int, (Int, Int))]  -- start, end (exclusive), captures

mSeq :: [RNode] -> String -> Int -> Int -> [(Int, (Int, Int))] -> (Int -> [(Int, (Int, Int))] -> Maybe MRes) -> Maybe MRes
mSeq [] _ _ pos caps k = k pos caps
mSeq (n : ns) s len pos caps k =
  mNode n s len pos caps (\p c -> mSeq ns s len p c k)

mNode :: RNode -> String -> Int -> Int -> [(Int, (Int, Int))] -> (Int -> [(Int, (Int, Int))] -> Maybe MRes) -> Maybe MRes
mNode (RLit b) s len pos caps k =
  if pos <= len && strByte s pos == b then k (pos + 1) caps else Nothing
mNode RAny s len pos caps k =
  if pos <= len then k (pos + 1) caps else Nothing
mNode (RClass neg items) s len pos caps k =
  if pos <= len && classHit neg items (strByte s pos) then k (pos + 1) caps else Nothing
mNode RStart s len pos caps k =
  if pos == 1 then k pos caps else Nothing
mNode REnd s len pos caps k =
  if pos == len + 1 then k pos caps else Nothing
mNode (RGroup gi ns) s len pos caps k =
  mSeq ns s len pos caps (\p c -> k p ((gi, (pos, p)) : c))
mNode (RStar n) s len pos caps k = mStar n s len pos caps k
mNode (RRep n lo hi) s len pos caps k = mRep n lo hi 0 s len pos caps k
mNode (RBack gi) s len pos caps k =
  case lookupCap gi caps of
    Nothing -> k pos caps          -- group never matched: backref matches ""
    Just (st, en) ->
      let l = en - st
      in if pos + l - 1 <= len && strSub s pos (pos + l - 1) == strSub s st (en - 1)
         then k (pos + l) caps
         else Nothing

-- Greedy star: consume another iteration first, fall back to the
-- continuation. An iteration that consumes nothing is rejected to keep
-- termination (matching the empty string zero or infinity times is the same).
mStar :: RNode -> String -> Int -> Int -> [(Int, (Int, Int))] -> (Int -> [(Int, (Int, Int))] -> Maybe MRes) -> Maybe MRes
mStar n s len pos caps k =
  case mNode n s len pos caps (\p c -> if p == pos then Nothing else mStar n s len p c k) of
    Just r -> Just r
    Nothing -> k pos caps

mRep :: RNode -> Int -> Int -> Int -> String -> Int -> Int -> [(Int, (Int, Int))] -> (Int -> [(Int, (Int, Int))] -> Maybe MRes) -> Maybe MRes
mRep n lo hi cnt s len pos caps k =
  if cnt < lo
  then mNode n s len pos caps (\p c -> mRep n lo hi (cnt + 1) s len p c k)
  else if hi >= 0 && cnt >= hi
  then k pos caps
  else
    case mNode n s len pos caps (\p c -> if p == pos then Nothing else mRep n lo hi (cnt + 1) s len p c k) of
      Just r -> Just r
      Nothing -> k pos caps

classHit :: Bool -> [CItem] -> Int -> Bool
classHit neg items b =
  let hit = itemsHit items b
  in if neg then not hit else hit

itemsHit :: [CItem] -> Int -> Bool
itemsHit [] _ = False
itemsHit (CByte x : rest) b = b == x || itemsHit rest b
itemsHit (CRange lo hi : rest) b = (b >= lo && b <= hi) || itemsHit rest b

lookupCap :: Int -> [(Int, (Int, Int))] -> Maybe (Int, Int)
lookupCap _ [] = Nothing
lookupCap gi ((g, r) : rest) = if g == gi then Just r else lookupCap gi rest

-- Leftmost match at or after `from`. A pattern anchored with a leading ^
-- fails outright once position `from` fails — ^ cannot match later.
reSearchFrom :: RE -> String -> Int -> Maybe Found
reSearchFrom (RE ns _) s from = searchGo ns s (strLen s) from

searchGo :: [RNode] -> String -> Int -> Int -> Maybe Found
searchGo ns s len st =
  if st > len + 1
  then Nothing
  else case mSeq ns s len st [] (\p c -> Just (MRes p c)) of
         Just (MRes e c) -> Just (Found st e c)
         Nothing ->
           case ns of
             (RStart : _) -> Nothing
             _ -> searchGo ns s len (st + 1)

reTest :: RE -> String -> Bool
reTest re s =
  case reSearchFrom re s 1 of
    Just _ -> True
    Nothing -> False

-- === Replacement text ======================================================
-- Parsed once at script-parse time into tokens; literal runs are collapsed.

data RTok = TLit String | TWhole | TRef Int

parseRepl :: String -> [RTok]
parseRepl r = replGo r 1 (strLen r) "" []

replGo :: String -> Int -> Int -> String -> [RTok] -> [RTok]
replGo r i len lit toks =
  if i > len
  then reverse (flushLit lit toks)
  else
    let b = strByte r i
    in if b == 38                                                         -- &
       then replGo r (i + 1) len "" (TWhole : flushLit lit toks)
       else if b == 92                                                    -- \
       then if i + 1 > len
            then replGo r (i + 1) len (lit <> "\\") toks    -- lone trailing \
            else
              let b2 = strByte r (i + 1)
              in if b2 >= 49 && b2 <= 57
                 then replGo r (i + 2) len "" (TRef (b2 - 48) : flushLit lit toks)
                 else if b2 == 48
                 then replGo r (i + 2) len "" (TWhole : flushLit lit toks)  -- \0
                 else if b2 == 110 then replGo r (i + 2) len (lit <> "\n") toks
                 else if b2 == 116 then replGo r (i + 2) len (lit <> "\t") toks
                 else if b2 == 114 then replGo r (i + 2) len (lit <> "\r") toks
                 else if b2 == 97 then replGo r (i + 2) len (lit <> strChar 7) toks
                 else if b2 == 102 then replGo r (i + 2) len (lit <> strChar 12) toks
                 else if b2 == 118 then replGo r (i + 2) len (lit <> strChar 11) toks
                 else replGo r (i + 2) len (lit <> strChar b2) toks
                   -- \& \\ and backslash-newline all land here: literal next byte
       else replGo r (i + 1) len (lit <> strChar b) toks

flushLit :: String -> [RTok] -> [RTok]
flushLit lit toks = if lit == "" then toks else TLit lit : toks

maxRef :: [RTok] -> Int
maxRef [] = 0
maxRef (TRef n : rest) = let m = maxRef rest in if n > m then n else m
maxRef (_ : rest) = maxRef rest

expandRepl :: [RTok] -> String -> Found -> String
expandRepl toks s f = expandGo toks s f ""

expandGo :: [RTok] -> String -> Found -> String -> String
expandGo [] _ _ acc = acc
expandGo (TLit l : rest) s f acc = expandGo rest s f (acc <> l)
expandGo (TWhole : rest) s f acc =
  case f of
    Found st en _ -> expandGo rest s f (acc <> strSub s st (en - 1))
expandGo (TRef gi : rest) s f acc =
  case f of
    Found _ _ caps ->
      case lookupCap gi caps of
        Nothing -> expandGo rest s f acc
        Just (a, b) -> expandGo rest s f (acc <> strSub s a (b - 1))

-- === Substitution engine ===================================================
-- Handles g / p / Nth-occurrence selection in one walk. GNU sed forbids an
-- empty match immediately adjacent to the previous match, otherwise `x*`
-- would fire twice at each position; `prevEnd` tracks the position just past
-- the previous match (0 = none, positions are >= 1).

substitute :: RE -> [RTok] -> Bool -> Int -> String -> (String, Bool)
substitute re toks glob nth s =
  subGo re toks glob nth s 1 (strLen s) 0 0 "" False

subGo :: RE -> [RTok] -> Bool -> Int -> String -> Int -> Int -> Int -> Int -> String -> Bool -> (String, Bool)
subGo re toks glob nth s pos len prevEnd cnt acc changed =
  if pos > len + 1
  then (acc, changed)
  else case reSearchFrom re s pos of
    Nothing -> (acc <> strSub s pos len, changed)
    Just (Found st en caps) ->
      if en == st && st == prevEnd
      then -- empty match adjacent to the previous one: emit one literal byte
           -- (pos == st here) and continue past it, keeping prevEnd.
           if st > len
           then (acc <> strSub s pos len, changed)
           else subGo re toks glob nth s (st + 1) len prevEnd cnt (acc <> strSub s pos st) changed
      else
        let cnt2 = cnt + 1
            gap = strSub s pos (st - 1)
            sel = if glob then cnt2 >= nth else cnt2 == nth
        in if sel
           then
             let rep = expandRepl toks s (Found st en caps)
             in if en == st
                then if st > len
                     then (acc <> gap <> rep, True)
                     else if glob
                     then subGo re toks glob nth s (st + 1) len st cnt2 (acc <> gap <> rep <> strSub s st st) True
                     else (acc <> gap <> rep <> strSub s st len, True)
                else if glob
                then subGo re toks glob nth s en len en cnt2 (acc <> gap <> rep) True
                else (acc <> gap <> rep <> strSub s en len, True)
           else -- before the Nth occurrence: copy the match untouched, keep counting
             if en == st
             then if st > len
                  then (acc <> gap, changed)
                  else subGo re toks glob nth s (st + 1) len st cnt2 (acc <> gap <> strSub s st st) changed
             else subGo re toks glob nth s en len en cnt2 (acc <> gap <> strSub s st (en - 1)) changed

-- === Transliteration (y) ===================================================

translit :: String -> String -> String -> String
translit src dst s = transGo src dst s 1 (strLen s) ""

transGo :: String -> String -> String -> Int -> Int -> String -> String
transGo src dst s i len acc =
  if i > len
  then acc
  else
    let b = strByte s i
        j = findByte src b 1 (strLen src)
    in if j == 0
       then transGo src dst s (i + 1) len (acc <> strChar b)
       else transGo src dst s (i + 1) len (acc <> strSub dst j j)

findByte :: String -> Int -> Int -> Int -> Int
findByte src b i len =
  if i > len then 0
  else if strByte src i == b then i
  else findByte src b (i + 1) len

-- === Script representation =================================================

data Addr = ANone
  | ALine Int
  | ALast
  | ARe RE
  | AReLast          -- empty //: the most recently applied regex

data Cmd = CBlock                       -- {
  | CBlockEnd                           -- }
  | CSub (Maybe RE) [RTok] Bool Bool Int  -- pattern (Nothing = last regex), repl, g, p, Nth
  | CPrint                              -- p
  | CDelete                             -- d
  | CQuit Bool Int                      -- q / Q (print pattern space?, exit code)
  | CHold                               -- h
  | CHoldA                              -- H
  | CGet                                -- g
  | CGetA                               -- G
  | CXchg                               -- x
  | CZap                                -- z (GNU): clear pattern space
  | CLineNum                            -- =
  | CIns String                         -- i: emit text immediately
  | CApp String                         -- a: queue text for end of cycle
  | CTrans String String                -- y
  | CBra String                         -- b ("" = jump to end of script)
  | CTst String                         -- t: branch if substituted, reset flag
  | CTstNeg String                      -- T: branch if NOT substituted
  | CLbl String                         -- :label

data Instr = Instr Addr Bool Cmd        -- address, negated?, command

-- === Script parser =========================================================

parseScript :: String -> Either String [Instr]
parseScript s =
  case scanCmds s 1 (strLen s) [] of
    Left err -> Left err
    Right is ->
      case checkProgram is of
        Left err -> Left err
        Right _ -> Right is

scanCmds :: String -> Int -> Int -> [Instr] -> Either String [Instr]
scanCmds s i len acc =
  let j = skipSep s i len
  in if j > len
     then Right (reverse acc)
     else if strByte s j == 35                                            -- #
     then scanCmds s (skipLine s j len) len acc
     else case scanOne s j len of
            Left err -> Left err
            Right (ins, k) -> scanCmds s k len (ins : acc)

-- Inter-command separators: `;`, space, tab, newline, CR.
skipSep :: String -> Int -> Int -> Int
skipSep s i len =
  if i > len
  then i
  else
    let b = strByte s i
    in if b == 59 || b == 32 || b == 9 || b == 10 || b == 13
       then skipSep s (i + 1) len
       else i

skipBlank :: String -> Int -> Int -> Int
skipBlank s i len =
  if i > len
  then i
  else
    let b = strByte s i
    in if b == 32 || b == 9
       then skipBlank s (i + 1) len
       else i

skipLine :: String -> Int -> Int -> Int
skipLine s i len =
  if i > len then i
  else if strByte s i == 10 then i + 1
  else skipLine s (i + 1) len

scanOne :: String -> Int -> Int -> Either String (Instr, Int)
scanOne s i len =
  case scanAddr s i len of
    Left err -> Left err
    Right (addr, j0) ->
      if j0 <= len && strByte s j0 == 44                                  -- ,
      then Left "two-address ranges (addr1,addr2) are not supported"
      else
        let j1 = skipBlank s j0 len
            neg = j1 <= len && strByte s j1 == 33                         -- !
            j = skipBlank s (if neg then j1 + 1 else j1) len
        in if j > len
           then Left "expected a command"
           else scanCmd s j len addr neg

scanAddr :: String -> Int -> Int -> Either String (Addr, Int)
scanAddr s i len =
  if i > len
  then Right (ANone, i)
  else
    let b = strByte s i
    in if isDigit b
       then case readNum s i len 0 of
              (n, j) ->
                if j <= len && strByte s j == 126                         -- ~
                then Left "step addresses (first~step) are not supported"
                else Right (ALine n, j)
       else if b == 36 then Right (ALast, i + 1)                          -- $
       else if b == 47 then scanReAddr s (i + 1) len 47                   -- /re/
       else if b == 92                                                    -- \cREc
       then if i + 1 > len
            then Left "expected a delimiter after \\ in address"
            else scanReAddr s (i + 2) len (strByte s (i + 1))
       else Right (ANone, i)

scanReAddr :: String -> Int -> Int -> Int -> Either String (Addr, Int)
scanReAddr s i len d =
  case readField s i len d of
    Left err -> Left err
    Right (pat, j) ->
      if pat == ""
      then Right (AReLast, j)
      else case compileRe pat of
             Left err -> Left ("bad regex in address: " <> err)
             Right re -> Right (ARe re, j)

scanCmd :: String -> Int -> Int -> Addr -> Bool -> Either String (Instr, Int)
scanCmd s i len addr neg =
  let b = strByte s i
  in if b == 123 then Right (Instr addr neg CBlock, i + 1)                -- {
     else if b == 125                                                     -- }
     then if addrIsNone addr && not neg
          then Right (Instr ANone False CBlockEnd, i + 1)
          else Left "} takes no address"
     else if b == 115 then scanSubst s (i + 1) len addr neg               -- s
     else if b == 121 then scanTrans s (i + 1) len addr neg               -- y
     else if b == 112 then Right (Instr addr neg CPrint, i + 1)           -- p
     else if b == 100 then Right (Instr addr neg CDelete, i + 1)          -- d
     else if b == 113 then scanQuit s (i + 1) len addr neg True           -- q
     else if b == 81 then scanQuit s (i + 1) len addr neg False           -- Q
     else if b == 104 then Right (Instr addr neg CHold, i + 1)            -- h
     else if b == 72 then Right (Instr addr neg CHoldA, i + 1)            -- H
     else if b == 103 then Right (Instr addr neg CGet, i + 1)             -- g
     else if b == 71 then Right (Instr addr neg CGetA, i + 1)             -- G
     else if b == 120 then Right (Instr addr neg CXchg, i + 1)            -- x
     else if b == 122 then Right (Instr addr neg CZap, i + 1)             -- z
     else if b == 61 then Right (Instr addr neg CLineNum, i + 1)          -- =
     else if b == 105 then scanText s (i + 1) len addr neg True           -- i
     else if b == 97 then scanText s (i + 1) len addr neg False           -- a
     else if b == 98                                                      -- b
     then case scanLabelArg s (i + 1) len of
            (l, j) -> Right (Instr addr neg (CBra l), j)
     else if b == 116                                                     -- t
     then case scanLabelArg s (i + 1) len of
            (l, j) -> Right (Instr addr neg (CTst l), j)
     else if b == 84                                                      -- T
     then case scanLabelArg s (i + 1) len of
            (l, j) -> Right (Instr addr neg (CTstNeg l), j)
     else if b == 58                                                      -- :
     then if addrIsNone addr && not neg
          then case scanLabelArg s (i + 1) len of
                 (l, j) ->
                   if l == ""
                   then Left "a label name is required after :"
                   else Right (Instr ANone False (CLbl l), j)
          else Left ": (label) takes no address"
     else if isUnimplemented b
     then Left ("command '" <> strChar b <> "' is not implemented in sed.mll")
     else Left ("unknown command: " <> strChar b)

addrIsNone :: Addr -> Bool
addrIsNone ANone = True
addrIsNone _ = False

isUnimplemented :: Int -> Bool
isUnimplemented b =
  b == 110 || b == 78 || b == 80 || b == 68 || b == 99 || b == 119        -- n N P D c w
  || b == 114 || b == 82 || b == 87 || b == 108 || b == 101 || b == 70    -- r R W l e F
  || b == 118                                                             -- v

-- b/t/T label argument (may be empty). Terminated by newline, `;` or `}`
-- (GNU sed behavior, confirmed against gsed 4.10); surrounding blanks trimmed.
scanLabelArg :: String -> Int -> Int -> (String, Int)
scanLabelArg s i len = labelGo s (skipBlank s i len) len ""

labelGo :: String -> Int -> Int -> String -> (String, Int)
labelGo s i len acc =
  if i > len
  then (trimEnd acc, i)
  else
    let b = strByte s i
    in if b == 10 || b == 13 || b == 59 || b == 125
       then (trimEnd acc, i)
       else labelGo s (i + 1) len (acc <> strChar b)

trimEnd :: String -> String
trimEnd s =
  let l = strLen s
  in if l >= 1 && (strByte s l == 32 || strByte s l == 9)
     then trimEnd (strSub s 1 (l - 1))
     else s

-- q / Q with an optional exit code.
scanQuit :: String -> Int -> Int -> Addr -> Bool -> Bool -> Either String (Instr, Int)
scanQuit s i len addr neg doPrint =
  let j = skipBlank s i len
  in if j <= len && isDigit (strByte s j)
     then case readNum s j len 0 of
            (code, k) -> Right (Instr addr neg (CQuit doPrint code), k)
     else Right (Instr addr neg (CQuit doPrint 0), i)

-- i/a text. Three forms, all GNU-compatible (probed against gsed 4.10):
--   i\<newline>text-lines     leading whitespace of text lines preserved
--   i\text                    text starts right after the backslash
--   i text                    leading blanks stripped
-- Text lines ending in \ continue on the next line; \t etc. are processed.
scanText :: String -> Int -> Int -> Addr -> Bool -> Bool -> Either String (Instr, Int)
scanText s i len addr neg isIns =
  let j = skipBlank s i len
      start = if j <= len && strByte s j == 92
              then if j + 1 <= len && strByte s (j + 1) == 10
                   then j + 2
                   else j + 1
              else j
  in case collectText s start len "" of
       (txt, k) ->
         if isIns
         then Right (Instr addr neg (CIns txt), k)
         else Right (Instr addr neg (CApp txt), k)

collectText :: String -> Int -> Int -> String -> (String, Int)
collectText s i len acc =
  if i > len
  then (acc, i)
  else
    let b = strByte s i
    in if b == 10
       then (acc, i + 1)
       else if b == 92
       then if i + 1 > len
            then (acc <> "\\", i + 1)
            else
              let b2 = strByte s (i + 1)
              in if b2 == 10 then collectText s (i + 2) len (acc <> "\n")
                 else if b2 == 110 then collectText s (i + 2) len (acc <> "\n")
                 else if b2 == 116 then collectText s (i + 2) len (acc <> "\t")
                 else if b2 == 114 then collectText s (i + 2) len (acc <> "\r")
                 else collectText s (i + 2) len (acc <> strChar b2)
       else collectText s (i + 1) len (acc <> strChar b)

-- s<D>regex<D>replacement<D>flags
scanSubst :: String -> Int -> Int -> Addr -> Bool -> Either String (Instr, Int)
scanSubst s i len addr neg =
  if i > len
  then Left "s command is missing its delimiter"
  else
    let d = strByte s i
    in if d == 10 || d == 92
       then Left "s command delimiter must not be newline or backslash"
       else case readField s (i + 1) len d of
              Left err -> Left err
              Right (pat, j) ->
                case readField s j len d of
                  Left err -> Left err
                  Right (repl, k) ->
                    case scanSFlags s k len False False 0 of
                      Left err -> Left err
                      Right (glob, pf, nth, m) ->
                        let toks = parseRepl repl
                        in if pat == ""
                           then Right (Instr addr neg (CSub Nothing toks glob pf (if nth == 0 then 1 else nth)), m)
                           else case compileRe pat of
                                  Left err -> Left ("bad regex: " <> err)
                                  Right re ->
                                    case re of
                                      RE _ groups ->
                                        if maxRef toks > groups
                                        then Left ("replacement refers to group \\" <> show (maxRef toks)
                                                   <> " but the pattern only has " <> show groups <> " group(s)")
                                        else Right (Instr addr neg (CSub (Just re) toks glob pf (if nth == 0 then 1 else nth)), m)

scanSFlags :: String -> Int -> Int -> Bool -> Bool -> Int -> Either String (Bool, Bool, Int, Int)
scanSFlags s i len glob pf nth =
  if i > len
  then Right (glob, pf, nth, i)
  else
    let b = strByte s i
    in if b == 103 then scanSFlags s (i + 1) len True pf nth               -- g
       else if b == 112 then scanSFlags s (i + 1) len glob True nth        -- p
       else if isDigit b
       then case readNum s i len 0 of
              (n, j) ->
                if n == 0
                then Left "the s command's numeric flag may not be zero"
                else scanSFlags s j len glob pf n
       else if b == 119 || b == 101 || b == 109 || b == 77 || b == 105 || b == 73
       then Left ("s flag '" <> strChar b <> "' is not supported")          -- w e m M i I
       else Right (glob, pf, nth, i)

-- y<D>source<D>dest<D>
scanTrans :: String -> Int -> Int -> Addr -> Bool -> Either String (Instr, Int)
scanTrans s i len addr neg =
  if i > len
  then Left "y command is missing its delimiter"
  else
    let d = strByte s i
    in case readYField s (i + 1) len d "" of
         Left err -> Left err
         Right (src, j) ->
           case readYField s j len d "" of
             Left err -> Left err
             Right (dst, k) ->
               if strLen src == strLen dst
               then Right (Instr addr neg (CTrans src dst), k)
               else Left "y: the two strings must have the same length"

-- y fields carry no regex syntax, so escapes decode to bytes right here:
-- \<delim> \\ \n \t \r; any other \x is the literal x.
readYField :: String -> Int -> Int -> Int -> String -> Either String (String, Int)
readYField s i len d acc =
  if i > len
  then Left "unterminated y command (missing delimiter)"
  else
    let b = strByte s i
    in if b == d
       then Right (acc, i + 1)
       else if b == 92
       then if i + 1 > len
            then Left "trailing backslash in y command"
            else
              let b2 = strByte s (i + 1)
              in if b2 == d then readYField s (i + 2) len d (acc <> strChar d)
                 else if b2 == 110 then readYField s (i + 2) len d (acc <> "\n")
                 else if b2 == 116 then readYField s (i + 2) len d (acc <> "\t")
                 else if b2 == 114 then readYField s (i + 2) len d (acc <> "\r")
                 else readYField s (i + 2) len d (acc <> strChar b2)
       else readYField s (i + 1) len d (acc <> strChar b)

-- Read a delimited field, consuming the closing delimiter and returning the
-- index just past it. `\<D>` collapses to a literal delimiter byte; every
-- OTHER `\x` pair is preserved verbatim so regex escapes (\(, \n, \1) and
-- replacement escapes (\n, \&, \\, backslash-newline) survive intact.
readField :: String -> Int -> Int -> Int -> Either String (String, Int)
readField s i len d = readFieldAcc s i len d ""

readFieldAcc :: String -> Int -> Int -> Int -> String -> Either String (String, Int)
readFieldAcc s i len d acc =
  if i > len
  then Left "unterminated field (missing delimiter)"
  else
    let b = strByte s i
    in if b == d
       then Right (acc, i + 1)
       else if b == 92
       then if i + 1 > len
            then Left "trailing backslash in field"
            else
              let b2 = strByte s (i + 1)
              in if b2 == d
                 then readFieldAcc s (i + 2) len d (acc <> strChar d)
                 else readFieldAcc s (i + 2) len d (acc <> strChar 92 <> strChar b2)
       else readFieldAcc s (i + 1) len d (acc <> strChar b)

-- Whole-program checks, mirroring GNU sed's parse-time errors: braces must
-- balance and every branch target must exist.
checkProgram :: [Instr] -> Either String ()
checkProgram is =
  case checkBraces is 0 of
    Left err -> Left err
    Right _ -> checkLabels is (collectLabels is)

checkBraces :: [Instr] -> Int -> Either String ()
checkBraces [] d = if d == 0 then Right () else Left "unmatched {"
checkBraces (Instr _ _ CBlock : rest) d = checkBraces rest (d + 1)
checkBraces (Instr _ _ CBlockEnd : rest) d =
  if d == 0 then Left "unexpected }" else checkBraces rest (d - 1)
checkBraces (_ : rest) d = checkBraces rest d

collectLabels :: [Instr] -> [String]
collectLabels [] = []
collectLabels (Instr _ _ (CLbl l) : rest) = l : collectLabels rest
collectLabels (_ : rest) = collectLabels rest

checkLabels :: [Instr] -> [String] -> Either String ()
checkLabels [] _ = Right ()
checkLabels (Instr _ _ (CBra l) : rest) ls = checkLabelRef l ls rest
checkLabels (Instr _ _ (CTst l) : rest) ls = checkLabelRef l ls rest
checkLabels (Instr _ _ (CTstNeg l) : rest) ls = checkLabelRef l ls rest
checkLabels (_ : rest) ls = checkLabels rest ls

checkLabelRef :: String -> [String] -> [Instr] -> Either String ()
checkLabelRef l ls rest =
  if l == "" || elem l ls
  then checkLabels rest ls
  else Left ("can't find label for jump to '" <> l <> "'")

-- === Execution machine =====================================================

-- Lazy input lookahead: nothing is read until the cycle needs a line, and a
-- `$` address forces at most one line of peek-ahead. Interactive scripts
-- without `$` therefore never block on a future line.
data Inp = IUnknown | IPend String | IEOF

data St = St
  { stPat :: String        -- pattern space
  , stHold :: String       -- hold space
  , stLine :: Int          -- current input line number
  , stTF :: Bool           -- substitution-since-last-line-or-t flag
  , stLastRe :: Maybe RE   -- most recently applied regex (for // and s//)
  , stIn :: Inp
  , stApp :: [String]      -- queued a\ texts, flushed at end of cycle
  }

-- How a cycle ended: fell off the end (auto-print applies), was deleted by
-- d (no auto-print), or quit (print per q/Q, then stop; Int = exit code).
data Out = ONorm St | ODel St | OQuit Bool Int St

emitLine :: String -> IO ()
emitLine s = do
  putStr s
  putStr "\n"

takeLine :: St -> IO (Maybe String, St)
takeLine st =
  case stIn st of
    IEOF -> pure (Nothing, st)
    IPend l -> pure (Just l, st { stIn = IUnknown })
    IUnknown -> do
      r <- try getLine
      case r of
        Left _ -> pure (Nothing, st { stIn = IEOF })
        Right l -> pure (Just l, st)

isLastLine :: St -> IO (Bool, St)
isLastLine st =
  case stIn st of
    IEOF -> pure (True, st)
    IPend _ -> pure (False, st)
    IUnknown -> do
      r <- try getLine
      case r of
        Left _ -> pure (True, st { stIn = IEOF })
        Right l -> pure (False, st { stIn = IPend l })

-- Evaluating a regex address registers it as the "last regex" (GNU
-- semantics: // and s// refer to the most recently *used* regex, whether or
-- not it matched — confirmed against gsed 4.10).
addrMatch :: Addr -> St -> IO (Bool, St)
addrMatch ANone st = pure (True, st)
addrMatch (ALine n) st = pure (stLine st == n, st)
addrMatch ALast st = isLastLine st
addrMatch (ARe re) st = pure (reTest re (stPat st), st { stLastRe = Just re })
addrMatch AReLast st =
  case stLastRe st of
    Nothing -> error "sed: no previous regular expression for empty // address"
    Just re -> pure (reTest re (stPat st), st)

-- Skip a { whose address did not match: drop everything up to and including
-- the matching }, minding nesting.
skipBlock :: [Instr] -> Int -> [Instr]
skipBlock [] _ = []
skipBlock (Instr _ _ CBlockEnd : rest) d =
  if d == 0 then rest else skipBlock rest (d - 1)
skipBlock (Instr _ _ CBlock : rest) d = skipBlock rest (d + 1)
skipBlock (_ : rest) d = skipBlock rest d

-- Resume after a label (parse-time validation guarantees it exists).
afterLabel :: [Instr] -> String -> [Instr]
afterLabel [] _ = []
afterLabel (Instr _ _ (CLbl l2) : rest) l = if l2 == l then rest else afterLabel rest l
afterLabel (_ : rest) l = afterLabel rest l

execI :: [Instr] -> [Instr] -> St -> IO Out
execI _ [] st = pure (ONorm st)
execI prog (Instr addr neg cmd : rest) st =
  case cmd of
    CLbl _ -> execI prog rest st
    CBlockEnd -> execI prog rest st
    _ -> do
      (m0, st1) <- addrMatch addr st
      let m = if neg then not m0 else m0
      if m
      then doCmd prog cmd rest st1
      else case cmd of
             CBlock -> execI prog (skipBlock rest 0) st1
             _ -> execI prog rest st1

doCmd :: [Instr] -> Cmd -> [Instr] -> St -> IO Out
doCmd prog cmd rest st =
  case cmd of
    CBlock -> execI prog rest st
    CBlockEnd -> execI prog rest st
    CLbl _ -> execI prog rest st
    CPrint -> do
      emitLine (stPat st)
      execI prog rest st
    CDelete -> pure (ODel st)
    CQuit doPrint code -> pure (OQuit doPrint code st)
    CHold -> execI prog rest (st { stHold = stPat st })
    CHoldA -> execI prog rest (st { stHold = stHold st <> "\n" <> stPat st })
    CGet -> execI prog rest (st { stPat = stHold st })
    CGetA -> execI prog rest (st { stPat = stPat st <> "\n" <> stHold st })
    CXchg -> execI prog rest (st { stPat = stHold st, stHold = stPat st })
    CZap -> execI prog rest (st { stPat = "" })
    CLineNum -> do
      putStrLn (show (stLine st))
      execI prog rest st
    CIns txt -> do
      emitLine txt
      execI prog rest st
    CApp txt -> execI prog rest (st { stApp = stApp st ++ [txt] })
    CTrans src dst -> execI prog rest (st { stPat = translit src dst (stPat st) })
    CBra l ->
      if l == ""
      then pure (ONorm st)
      else execI prog (afterLabel prog l) st
    CTst l ->
      if stTF st
      then branchReset prog l st
      else execI prog rest st
    CTstNeg l ->
      if stTF st
      then execI prog rest (st { stTF = False })
      else branchReset prog l st
    CSub mre toks glob pf nth ->
      let re = case mre of
                 Just r -> r
                 Nothing ->
                   case stLastRe st of
                     Just r -> r
                     Nothing -> error "sed: no previous regular expression for s//"
      in case substitute re toks glob nth (stPat st) of
           (s2, changed) -> do
             when (changed && pf) (emitLine s2)
             execI prog rest (st { stPat = s2
                                 , stTF = stTF st || changed
                                 , stLastRe = Just re })

branchReset :: [Instr] -> String -> St -> IO Out
branchReset prog l st =
  let st2 = st { stTF = False }
  in if l == ""
     then pure (ONorm st2)
     else execI prog (afterLabel prog l) st2

flushApp :: St -> IO St
flushApp st = do
  emitAll (stApp st)
  pure (st { stApp = [] })

emitAll :: [String] -> IO ()
emitAll [] = pure ()
emitAll (x : xs) = do
  emitLine x
  emitAll xs

run :: [Instr] -> Bool -> St -> IO ()
run prog noAuto st = do
  (ml, st1) <- takeLine st
  case ml of
    Nothing -> pure ()
    Just line -> do
      let st2 = st1 { stPat = line, stLine = stLine st1 + 1, stTF = False }
      out <- execI prog prog st2
      case out of
        ONorm st3 -> do
          when (not noAuto) (emitLine (stPat st3))
          st4 <- flushApp st3
          flushStdout
          run prog noAuto st4
        ODel st3 -> do
          st4 <- flushApp st3
          flushStdout
          run prog noAuto st4
        OQuit doPrint code st3 -> do
          when (doPrint && not noAuto) (emitLine (stPat st3))
          _ <- flushApp st3
          flushStdout
          when (code /= 0) (exit (Err code))

initSt :: St
initSt = St { stPat = "", stHold = "", stLine = 0, stTF = False
            , stLastRe = Nothing, stIn = IUnknown, stApp = [] }

-- === Argument handling & main =============================================

readFileAll :: String -> IO String
readFileAll path = do
  r <- fOpen path "r"
  case r of
    Left err -> error ("sed: cannot open " <> path <> ": " <> err)
    Right h -> do
      c <- fRead h "*a"
      fClose h
      pure c

-- Walk argv collecting -n / -e / -f (clusters like -nf work) plus at most
-- one bare script operand. Input-file operands are not supported: this sed
-- reads stdin only.
gatherArgs :: [String] -> Bool -> [String] -> Bool -> IO (Either String (Bool, String))
gatherArgs [] noAuto parts scriptGiven =
  if null parts
  then pure (Left "usage: sed [-n] [-e script] [-f script-file] [script]")
  else pure (Right (noAuto, joinNL (reverse parts)))
gatherArgs (a : rest) noAuto parts scriptGiven =
  if a == "--"
  then bareOperands rest noAuto parts scriptGiven
  else if strLen a >= 2 && strByte a 1 == 45                              -- -x…
  then procFlags a 2 rest noAuto parts scriptGiven
  else bareOperands (a : rest) noAuto parts scriptGiven

bareOperands :: [String] -> Bool -> [String] -> Bool -> IO (Either String (Bool, String))
bareOperands [] noAuto parts scriptGiven = gatherArgs [] noAuto parts scriptGiven
bareOperands (a : rest) noAuto parts scriptGiven =
  if scriptGiven || not (null parts)
  then pure (Left ("input file operands are not supported (got '" <> a <> "'); sed.mll reads stdin"))
  else gatherArgs rest noAuto (a : parts) True

procFlags :: String -> Int -> [String] -> Bool -> [String] -> Bool -> IO (Either String (Bool, String))
procFlags a i rest noAuto parts scriptGiven =
  if i > strLen a
  then gatherArgs rest noAuto parts scriptGiven
  else
    let c = strByte a i
    in if c == 110                                                        -- n
       then procFlags a (i + 1) rest True parts scriptGiven
       else if c == 101                                                   -- e
       then if i + 1 <= strLen a
            then gatherArgs rest noAuto (strSub a (i + 1) (strLen a) : parts) True
            else case rest of
                   [] -> pure (Left "option -e requires a script argument")
                   (v : rest2) -> gatherArgs rest2 noAuto (v : parts) True
       else if c == 102                                                   -- f
       then if i + 1 <= strLen a
            then do
              c2 <- readFileAll (strSub a (i + 1) (strLen a))
              gatherArgs rest noAuto (c2 : parts) True
            else case rest of
                   [] -> pure (Left "option -f requires a file argument")
                   (v : rest2) -> do
                     c2 <- readFileAll v
                     gatherArgs rest2 noAuto (c2 : parts) True
       else pure (Left ("unknown option: -" <> strChar c))

joinNL :: [String] -> String
joinNL [] = ""
joinNL (x : []) = x
joinNL (x : xs) = x <> "\n" <> joinNL xs

-- A script whose first line is exactly `#n` enables -n (POSIX).
hashN :: String -> Bool
hashN s =
  strLen s >= 2 && strSub s 1 2 == "#n"
  && (strLen s == 2 || strByte s 3 == 10)

main :: IO ()
main = do
  args <- getArgs
  r <- gatherArgs args False [] False
  case r of
    Left err -> putStrLn ("sed: " <> err)
    Right (noAuto, script) ->
      case parseScript script of
        Left err -> putStrLn ("sed: " <> err)
        Right prog -> run prog (noAuto || hashN script) initSt
