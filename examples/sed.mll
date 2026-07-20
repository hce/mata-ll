-- sed.mll — a streaming, sed-like stream editor written in mata-ll.
--
-- WHAT IT IS
--   Reads stdin line by line and applies a sed-style script given as a
--   program argument. Processing is streaming with one line of lookahead,
--   so `$` (last-line) addressing works in O(1) memory — stdin is never
--   slurped in full.
--
-- SUPPORTED
--   * -n flag            suppress automatic printing of the pattern space
--   * multiple commands  separated by `;`, space, tab, newline or CR
--   * single addresses   N (line number), $ (last line), /regex/
--   * commands           s (substitute), p (print), d (delete)
--   * s flags            g (global), p (print on successful substitution)
--   * replacements       & = whole match; \n \t \r escapes; \& \\ \x literals
--
-- NOT YET SUPPORTED (parsed cleanly nowhere — simply absent)
--   * two-address ranges          e.g. 2,5d
--   * hold space                  h H g G x
--   * branching / labels          b t : label
--   * backreferences \1..\9       the Regex module has no capture groups
--
-- KNOWN DEVIATION FROM GNU/BSD sed
--   Every output line ends in a newline. Real sed preserves the ABSENCE of a
--   trailing newline on the last input line; we cannot, because getLine strips
--   the newline and does not report whether one was present. So `printf a`
--   yields `b\n` here but `b` (no newline) under real sed.
--
-- REGEX FLAVOUR (from the bundled Regex module — ERE-ish, no backrefs)
--   . * + ? | ( ) [ ] [^ ] ^ $  and the escapes \d \w \s \D \W \S \n \t \r
--
-- EXAMPLE INVOCATIONS (run in-tree; the mll binary auto-loads lib/)
--   printf 'foo\nbar\n' | target/release/mll examples/sed.mll -r s/foo/baz/
--   printf 'abc\n'      | target/release/mll examples/sed.mll -r s/x*/-/g
--   printf 'a\nb\nc\n'  | target/release/mll examples/sed.mll -r -n /b/p
--   printf 'a\nb\nc\n'  | target/release/mll examples/sed.mll -r '$d'
--
-- Everything after the script filename becomes getArgs, so mll's own flags
-- (like -r) go BEFORE the filename; the sed script and -n go after it.

import Regex
import LString (strByte, strLen, strSub, strChar)

-- === Data model ============================================================

data Addr  = ALine Integer | ALast | ARegex RE | ANone
data Flags = Flags Bool Bool          -- global?, printOnSub?
data Cmd   = Subst RE String Flags | CPrint | CDelete
data Command = Command Addr Cmd

-- Result of running one command against the pattern space. `Deleted` ends
-- the cycle immediately (no further commands, no auto-print).
data Step = Deleted | Continue String

-- Byte codes used throughout (mata-ll has no char literals):
--   59 ;   32 space   9 tab   10 \n   13 \r   36 $   47 /   92 \
--   115 s  112 p      100 d   103 g   38 &    110 n  116 t  114 r
--   48..57 = '0'..'9'

-- === Script parser =========================================================
-- Hand-written recursive parser over 1-based indices. Returns Either so a
-- malformed script surfaces a message instead of a crash.

parseScript :: String -> Either String [Command]
parseScript s = parseCmds s 1 (strLen s)

parseCmds :: String -> Integer -> Integer -> Either String [Command]
parseCmds s i len =
  let j = skipSep s i len
  in if j > len
     then Right []
     else case parseOne s j len of
            Left err -> Left err
            Right (cmd, k) -> case parseCmds s k len of
                                Left err   -> Left err
                                Right rest -> Right (cmd : rest)

-- Skip inter-command separators.
skipSep :: String -> Integer -> Integer -> Integer
skipSep s i len =
  if i > len then i
  else let b = strByte s i
       in if b == 59 || b == 32 || b == 9 || b == 10 || b == 13
          then skipSep s (i + 1) len
          else i

parseOne :: String -> Integer -> Integer -> Either String (Command, Integer)
parseOne s i len =
  case parseAddr s i len of
    Left err -> Left err
    Right (addr, j) -> case parseCmd s j len of
                         Left err        -> Left err
                         Right (cmd, k)  -> Right (Command addr cmd, k)

-- Optional address directly before the command character.
parseAddr :: String -> Integer -> Integer -> Either String (Addr, Integer)
parseAddr s i len =
  if i > len then Right (ANone, i)
  else let b = strByte s i
       in if b >= 48 && b <= 57
          then case readNum s i len 0 of
                 (n, j) -> Right (ALine n, j)
          else if b == 36 then Right (ALast, i + 1)
          else if b == 47
               then case readField s (i + 1) len 47 of
                      Left err -> Left err
                      Right (pat, j) -> case compile pat of
                        Left err -> Left ("bad regex in address: " <> err)
                        Right re -> Right (ARegex re, j)
               else Right (ANone, i)

readNum :: String -> Integer -> Integer -> Integer -> (Integer, Integer)
readNum s i len acc =
  if i > len then (acc, i)
  else let b = strByte s i
       in if b >= 48 && b <= 57
          then readNum s (i + 1) len (acc * 10 + (b - 48))
          else (acc, i)

parseCmd :: String -> Integer -> Integer -> Either String (Cmd, Integer)
parseCmd s i len =
  if i > len then Left "expected a command"
  else let b = strByte s i
       in if b == 115 then parseSubst s (i + 1) len   -- 's'
          else if b == 112 then Right (CPrint, i + 1)  -- 'p'
          else if b == 100 then Right (CDelete, i + 1) -- 'd'
          else Left ("unknown command: " <> strChar b)

-- s<D>regex<D>replacement<D>flags   where <D> is the byte right after 's'.
parseSubst :: String -> Integer -> Integer -> Either String (Cmd, Integer)
parseSubst s i len =
  if i > len then Left "s command is missing its delimiter"
  else let d = strByte s i
       in case readField s (i + 1) len d of
            Left err -> Left err
            Right (pat, j) -> case readField s j len d of
              Left err -> Left err
              Right (repl, k) -> case readFlags s k len (Flags False False) of
                (fl, m) -> case compile pat of
                  Left err -> Left ("bad regex: " <> err)
                  Right re -> Right (Subst re repl fl, m)

readFlags :: String -> Integer -> Integer -> Flags -> (Flags, Integer)
readFlags s i len (Flags g p) =
  if i > len then (Flags g p, i)
  else let b = strByte s i
       in if b == 103 then readFlags s (i + 1) len (Flags True p) -- 'g'
          else if b == 112 then readFlags s (i + 1) len (Flags g True) -- 'p'
          else (Flags g p, i)

-- Read a delimited field, consuming the closing delimiter and returning the
-- index just past it. `\<D>` collapses to a literal delimiter byte (the
-- backslash is dropped); every OTHER `\x` is preserved verbatim so that
-- regex escapes (\d) and replacement escapes (\n, \&, \\) survive intact.
readField :: String -> Integer -> Integer -> Integer -> Either String (String, Integer)
readField s i len d = readFieldAcc s i len d ""

readFieldAcc :: String -> Integer -> Integer -> Integer -> String -> Either String (String, Integer)
readFieldAcc s i len d acc =
  if i > len then Left "unterminated field (missing delimiter)"
  else let b = strByte s i
       in if b == d then Right (acc, i + 1)
          else if b == 92
               then if i + 1 > len
                    then Left "trailing backslash in field"
                    else let b2 = strByte s (i + 1)
                         in if b2 == d
                            then readFieldAcc s (i + 2) len d (acc <> strChar d)
                            else readFieldAcc s (i + 2) len d (acc <> strChar 92 <> strChar b2)
               else readFieldAcc s (i + 1) len d (acc <> strChar b)

-- === Replacement expansion =================================================
-- & -> whole match; \n \t \r -> control bytes; \& \\ \x -> the literal x.

expandRepl :: String -> String -> String
expandRepl repl matched = expandReplAcc repl 1 (strLen repl) matched ""

expandReplAcc :: String -> Integer -> Integer -> String -> String -> String
expandReplAcc repl i len matched acc =
  if i > len then acc
  else let b = strByte repl i
       in if b == 38 then expandReplAcc repl (i + 1) len matched (acc <> matched) -- '&'
          else if b == 92
               then if i + 1 > len
                    then acc <> strChar 92           -- lone trailing backslash
                    else let b2 = strByte repl (i + 1)
                             c = if b2 == 110 then strChar 10
                                 else if b2 == 116 then strChar 9
                                 else if b2 == 114 then strChar 13
                                 else strChar b2
                         in expandReplAcc repl (i + 2) len matched (acc <> c)
               else expandReplAcc repl (i + 1) len matched (acc <> strChar b)

-- === Substitution engine ===================================================

-- Replace only the leftmost match.
subst1 :: RE -> String -> String -> (String, Bool)
subst1 re repl s =
  case findFrom re s 1 (strLen s) of
    Nothing -> (s, False)
    Just (Match st ml) ->
      let before  = strSub s 1 (st - 1)
          matched = strSub s st (st + ml - 1)
          after   = strSub s (st + ml) (strLen s)
      in (before <> expandRepl repl matched <> after, True)

-- Replace every match. The tricky part is empty matches: GNU sed forbids an
-- empty match immediately adjacent to the previous match, otherwise `x*`
-- would fire twice at each position. `prevEnd` is the position just past the
-- previous match (0 means "no previous match", since positions are >= 1).
gsubst :: RE -> String -> String -> (String, Bool)
gsubst re repl s = gsubstGo re repl s 1 (strLen s) 0 "" False

gsubstGo :: RE -> String -> String -> Integer -> Integer -> Integer -> String -> Bool -> (String, Bool)
gsubstGo re repl s pos len prevEnd acc changed =
  if pos > len + 1 then (acc, changed)
  else case findFrom re s pos len of
    Nothing -> (acc <> strSub s pos len, changed)
    Just (Match st ml) ->
      if ml == 0 && st == prevEnd
      then -- empty match adjacent to the previous one: reject it, emit one
           -- literal char (here pos == st) and step past it, keeping prevEnd.
           if st > len
           then (acc <> strSub s pos len, changed)
           else gsubstGo re repl s (st + 1) len prevEnd (acc <> strSub s pos st) changed
      else let gap     = strSub s pos (st - 1)
               matched = strSub s st (st + ml - 1)
               rep     = expandRepl repl matched
           in if ml == 0
              then gsubstGo re repl s (st + 1) len (st + ml)
                     (acc <> gap <> rep <> strSub s st st) True
              else gsubstGo re repl s (st + ml) len (st + ml)
                     (acc <> gap <> rep) True

-- === Per-line cycle ========================================================

emitLine :: String -> IO ()
emitLine s = do
  putStr s
  putStr (strChar 10)   -- getLine stripped the newline; re-add exact byte

addrMatch :: Addr -> String -> Integer -> Bool -> Bool
addrMatch ANone       _  _       _      = True
addrMatch (ALine k)   _  lineNum _      = lineNum == k
addrMatch ALast       _  _       isLast = isLast
addrMatch (ARegex re) ps _       _      = test re ps

runCmd :: Cmd -> String -> IO Step
runCmd CPrint  ps = do
  emitLine ps
  pure (Continue ps)
runCmd CDelete _  = pure Deleted
runCmd (Subst re repl (Flags g p)) ps =
  case (if g then gsubst re repl ps else subst1 re repl ps) of
    (ps', changed) -> do
      when (changed && p) (emitLine ps')
      pure (Continue ps')

applyOne :: Command -> String -> Integer -> Bool -> IO Step
applyOne (Command addr cmd) ps lineNum isLast =
  if addrMatch addr ps lineNum isLast
  then runCmd cmd ps
  else pure (Continue ps)

-- Run commands left-to-right; stop early on delete. Returns the final
-- pattern space and whether the line was deleted.
runCmds :: [Command] -> String -> Integer -> Bool -> IO (String, Bool)
runCmds [] ps _ _ = pure (ps, False)
runCmds (c : cs) ps lineNum isLast = do
  step <- applyOne c ps lineNum isLast
  case step of
    Deleted      -> pure (ps, True)
    Continue ps' -> runCmds cs ps' lineNum isLast

processLine :: Bool -> [Command] -> Integer -> String -> Bool -> IO ()
processLine noAuto cmds lineNum ps isLast = do
  (ps', deleted) <- runCmds cmds ps lineNum isLast
  when (not noAuto && not deleted) (emitLine ps')

-- === Streaming driver ======================================================
-- One line of lookahead: hold the current line, read the next; a failed read
-- (EOF) means the held line is the last one.

runStream :: Bool -> [Command] -> IO ()
runStream noAuto cmds = do
  r <- try getLine
  case r of
    Left _      -> pure ()
    Right first -> loop noAuto cmds 1 first

loop :: Bool -> [Command] -> Integer -> String -> IO ()
loop noAuto cmds lineNum cur = do
  r <- try getLine
  case r of
    Left _    -> processLine noAuto cmds lineNum cur True
    Right nxt -> do
      processLine noAuto cmds lineNum cur False
      loop noAuto cmds (lineNum + 1) nxt

-- === Argument handling & main =============================================

parseArgs :: [String] -> Either String (Bool, String)
parseArgs [] = Left "usage: sed [-n] script"
parseArgs (a : rest) =
  if a == "-n"
  then case rest of
         (script : _) -> Right (True, script)
         []           -> Left "usage: sed [-n] script (script missing after -n)"
  else Right (False, a)

main :: IO ()
main = do
  args <- getArgs
  case parseArgs args of
    Left err -> putStrLn ("sed: " <> err)
    Right (noAuto, script) -> case parseScript script of
      Left err   -> putStrLn ("sed: " <> err)
      Right cmds -> runStream noAuto cmds
