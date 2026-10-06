import Lake
open Lake DSL

package «tracelean» where

@[default_target]
lean_lib «TraceLean» where
  globs := #[.submodules `TraceLean]
