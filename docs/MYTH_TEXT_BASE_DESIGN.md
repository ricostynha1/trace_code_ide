# These were ideas from a  C project 
Where each text defined a grammar and then , depending on queries we have color, default tree sitter but better we could associsate functions to it and app functionality 

Text Formatter
The Text Formatter is a system to define and apply styles to text in a flexible way. Its purpose is to produce a simple structure that the frontend can use to render styled text, without needing to traverse the full Tree-sitter AST or handle query logic directly.
To define styles for a text, three elements are required:


A grammar that defines the text structure.


For example, if the text is JSON, we could pass:
const TSLanguage *tree_sitter_json(void);


Tree-sitter provides a wide range of pre-built grammars, and creating a custom one is straightforward. More details are on the Tree-sitter repo.




A JSON file defining styles for named captures:
{
    "@transitionFunction": { "color": "#0000ff" },
    "@state_name": { "color": "#00ff00" },
    "@key_sequence": {
        "color": "#ff0000",
        "underline": "1",
        "italic": "1",
        "bold": "1"
    }
}


The JSON allows adding new attributes freely and gives the frontend control over which ones to use.


Internally, the JSON can be parsed into a M2M_MAP_t structure. For example:
map.get("@state_name/color") // returns "#00ff00"




A Tree-sitter query scheme that maps AST nodes to capture names:
((transitionFunction) @transitionFunction)
((state_name) @state_name)
((key_sequence) @key_sequence)


More complex queries are also supported:
(class_declaration
  name: (identifier) @the-class-name
  body: (class_body
    (method_definition
      name: (property_identifier) @the-method-name)))


This allows associating nested structures (e.g., methods with their containing classes).
(TODO. For now only the simple captures are supported but this must be expanded)




Responsibilities of the Text Formatter
The Text Formatter combines these three inputs - grammar, JSON styles, and Tree-sitter queries - to build a structure that allows the frontend to render styled text efficiently. The core in-memory structure is:
typedef struct {
  artiststyle_array_t styles;  // dynamic array of style runs
  M2M_MAP_t *style_map;        // parsed JSON style map
  artiststyle_t default_style; // default style applied where no run exists
} byteToDesignInfo_t;

typedef struct {
  artiststyle_t *styles;
  size_t count;
  size_t cap;
} artiststyle_array_t;

typedef struct artiststyle {
  UINT32_t rel_start; // relative start position from previous run
  UINT32_t length;    // length of the run
  STR_t *jsonKey;     // pointer to the JSON key in style_map
} artiststyle_t;
Design reasoning for rel_start:

Inserting a new style between existing runs only requires memcpy to shift the later elements, without recomputing all subsequent start positions.
Text rendering is often local, so having runs tightly packed improves cache locality and speed.
Simplifies initial implementation while maintaining flexibility.

Limitations:

Insertions may still be expensive, especially since STR_t for the text buffer is also copied.
For highly editable text, alternative text structures (rope, gap buffer) may be considered in future iterations.
(TODO. This is a big caviat though)


Proposed API
artiststyle_t* get_next_style(byteToDesignInfo_t *bi);
void recompute_style(byteToDesignInfo_t *bi);
void local_update_style(byteToDesignInfo_t *bi, UINT32 start_pos, UINT32 end_pos, bool wasInsert);
artiststyle_t* get_style_on_pos(byteToDesignInfo_t *bi, UINT32 pos);

get_next_style() -> iterate through the text in order of runs.
recompute_style() -> recompute all styles from scratch.
local_update_style() -> only update runs affected by an insertion or deletion.
get_style_on_pos() -> retrieve the style at a given position, updating the internal cursor for subsequent calls.


(TODO. this API seems incomplete for now, will have to think more deeply in order to acoomodate case two on the bottom and complex querys)
Example Use Cases
1. Fully structured grammar (e.g., JSON)
Tree-sitter query:
((JSON_SYM_STRING) @json_string)
Styles:
{
    "@json_string": { "color": "#0000ff" }
}
Text buffer:
{
    "@json_string": {
        "color": "#0000ff"
    }
}
Behavior:

get_next_style() initially returns default style up to the first string.
The next call returns the style for @json_string (blue in this case).


2. Simple HTML-like color grammar
We define text of the form:
This is a phrase <color=#0000ff>this must be blue</color>
Tree-sitter grammar:
module.exports = grammar({
  name: 'color_markup',
  extras: $ => [],
  rules: {
    source_file: $ => repeat($._node),
    _node: $ => choice($.element, $.text),
    element: $ => seq(
      field('open_tag', $.open_tag),
      repeat($._node),
      field('close_tag', $.close_tag)
    ),
    open_tag: $ => seq('<', 'color', optional(/\s*/), '=', optional(/\s*),
                       field('color_value', $.hex_color), optional(/\s*), '>'),
    close_tag: $ => seq('</', 'color', '>'),
    hex_color: $ => /#[0-9A-Fa-f]{6}/,
    text: $ => /[^<]+/
  }
});
Query capturing color and content:
(element
  open_tag: (open_tag (hex_color) @color.value)
  close_tag: (close_tag)
  (text) @color.text
) @color.element


Captures:

@color.value -> the hex color
@color.text -> the text inside the tag
@color.element -> the full element (optional, for reference)



Mapping to JSON:
{
    "@color.text": { "color": "@color.value" }
}

This approach allows assigning styles to text spans directly based on the color tag.
The machinery to parse queries and build style runs is the same as for structured grammars - only the grammar and query differ.

(TODO: Currently, the frontend is using json as giving the values pure without need to look them up further, will need to change that in the builder creation)
MAJOR CONCERNS
As we are using a STR_T to hold text to be edited, this can become cobersome when editing large files requiring shifts etc, large text buffers.
We could reconsider this API to not receive STR_T as it is but receive a opaque pointer to something to hold the text but can be different.
But for that we have to define a good API. Doing that or thinking yet on a good option to hold text having this in mind.



# Part of the keyboard
This was a grammar that captured keyboard:
module.exports = grammar({
    name: 'keyboard',

    rules: {
			  keyboard_map: $ => repeat($._directives),

			  _directives: $ => choice (
					  $.comment_line,
					  $._rule
			  ),
			  _rule: $ => choice(
					  $.stateMap,
					  $.transitionState,
					  $.transitionFunction,
			  ),
    	  stateMap: $ => seq (
					  $.state_name,
					  ':'
		    ),

    	  transitionState: $ => seq (
					  '(',
					  $.key_sequence,
					  ')',
					  ':',
					  $.state_name
		    ),

    	  transitionFunction: $ => seq (
					  '(',
					  $.key_sequence,
					  ')',
					  ':',
					  $.funct_name
		    ),

        key_sequence: $ => choice(
            $._key,
            $._mod_key_seq
        ),

        _mod_key_seq: $=> seq (
					  $._mod_key,
					  $._key
        ),

			  comment_line: $ => /\/\/.+/,
			  _mod_key: $ => /C-|M-|A-|SUP-|S-/,
			  _key: $ => /[^-]|DEF|ESC|SPC|up|right|left|down|F1|F2|F3|F4|[0-9]-?[0-9]?/,
			  state_name: $ => /[A-Z][a-zA-Z0-9]*/,
			  funct_name: $ => /[a-z][a-zA-Z0-9]*/,
    }
});

# And then a keymap controlled What a keypress would do :
// State machine
// When pressing any key on the left trigger or go to onother menu (Uppper csae)
// Or calling the function that is inserted (lowercase)
// NM means -> No match
Main:
  (NM):Main
  (NM):char
  (ESC):Exit
  // States defined after main (notice that they are always upper case, aNM the key is upper)
  // Changes the state to INMice were almost all can me made (to do C-C you must C-i aNM then c)
  (C-SPC):Options
  (up):goUpLine
  (left):goLeftChar
  (down):goDownLine
  (right):goRightChar
  (S-up):goParent
  (S-left):goLeftSibling
  (S-down):goChild
  (S-right):goRightSibling
  (C-up):goLayoutUp
  (C-down):goLayoutDown
  (C-left):goLayoutLeft
  (C-right):goLayoutRight

Options:
  (ESC):Main
  (NM):Main
  (NM):true
  (C):Copy
  (V):Paste
  (Z):Undo
  (F):File
  (D):Document
  (S):Save
  (X):Cut
  (H):Help
  (M):Movement
  (B):Buffer
  (P):Page
  (A):Application
  // Most important functions of those states (notice that they are always lower case, aNM the key is lower)
  //triggers the copy function
  (c):copy
  (v):paste
  (z):undo
  // No actual state Y
  (y):redo
  (f):file
  (d):document
  (s):save
  (x):cut
  (h):help
  (m):moveCursor
  (b):bufferChange
  (p):changeNextPage
  (a):aplicationList
  //Movement occupies 4 plus arrows keys in shortcuts
  (i):goUpLine
  (k):goLeftChar
  (j):goDownLine
  (l):goRightChar
  // Movements inside a grammar tree are crucial in this application, allowing modal editing aNM precise selection of semantic components
  (I):goParent
  (J):goLeftSibling
  (K):goChild
  (L):goRightSibling
  (up):goUpLine
  (left):goLeftChar
  (down):goDownLine
  (right):goRightChar
// State machine
// When pressing any key on the left trigger or go to onother menu (Uppper csae)
// Or calling the function that is inserted (lowercase)
// NM means -> No match
Main:
  (NM):Main
  (NM):char
  (ESC):Exit
  // States defined after main (notice that they are always upper case, aNM the key is upper)
  // Changes the state to INMice were almost all can me made (to do C-C you must C-i aNM then c)
  (C-SPC):Options
  (up):goUpLine
  (left):goLeftChar
  (down):goDownLine
  (right):goRightChar
  (S-up):goParent
  (S-left):goLeftSibling
  (S-down):goChild
  (S-right):goRightSibling
  (C-up):goLayoutUp
  (C-down):goLayoutDown
  (C-left):goLayoutLeft
  (C-right):goLayoutRight

Options:
  (ESC):Main
  (NM):Main
  (NM):true
  (C):Copy
  (V):Paste
  (Z):Undo
  (F):File
  (D):Document
  (S):Save
  (X):Cut
  (H):Help
  (M):Movement
  (B):Buffer
  (P):Page
  (A):Application
  // Most important functions of those states (notice that they are always lower case, aNM the key is lower)
  //triggers the copy function
  (c):copy
  (v):paste
  (z):undo
  // No actual state Y
  (y):redo
  (f):file
  (d):document
  (s):save
  (x):cut
  (h):help
  (m):moveCursor
  (b):bufferChange
  (p):changeNextPage
  (a):aplicationList
  //Movement occupies 4 plus arrows keys in shortcuts
  (i):goUpLine
  (k):goLeftChar
  (j):goDownLine
  (l):goRightChar
  // Movements inside a grammar tree are crucial in this application, allowing modal editing aNM precise selection of semantic components
  (I):goParent
  (J):goLeftSibling
  (K):goChild
  (L):goRightSibling
  (up):goUpLine
  (left):goLeftChar
  (down):goDownLine
  (right):goRightChar
  (S-up):goParent
  (S-left):goLeftSibling
  (S-down):goChild
  (S-right):goRightSibling

Exit:
  (ESC):Main
  (NM):Main
  (NM):true
  // different key on purpose to avoid typos
  (q):exit

Copy:
  (ESC):Main
  (NM):Main
  (NM):true
   // Has the map copy is acesses with c, pressing other c calls the most used function of the map
  (c):copy
  (a):copyAllFile
  (n):copyFileName
  (p):copyPathName
  (s):copySectionName
  (r):copyRectangle
  (0-9):copyToRegister

Document:
  (ESC):Main
  (NM):Main
  (NM):true
  (d):opeNMocument
  (t):fileTree
  (s):saveFile
  (d):deleteFile
  (e):exitFile
  (r):renameFile
  (p):changePath
  (P):changePermissions

Paste:
  (ESC):Main
  (NM):Main
  (NM):true
  (v):paste
  (n):pasteFileName
  (p):pastePathName
  (s):pasteSectionName
  (0-9):pasteRegister

Cut:
  (ESC):Main
  (NM):Main
  (NM):true
  (x):cut
  (l):cutLine

Save:
  (ESC):Main
  (NM):Main
  (NM):true
  (s):save
  (f):saveAllFiles
  (a):saveAs
  (e):exportAs

Help:
  (ESC):Main
  (NM):Main
  (NM):true
  (h):chatBotHelper
  (d):documentation
  (S-up):goParent
  (S-left):goLeftSibling
  (S-down):goChild
  (S-right):goRightSibling

Exit:
  (ESC):Main
  (NM):Main
  (NM):true
  // different key on purpose to avoid typos
  (q):exit

Copy:
  (ESC):Main
  (NM):Main
  (NM):true
   // Has the map copy is acesses with c, pressing other c calls the most used function of the map
  (c):copy
  (a):copyAllFile
  (n):copyFileName
  (p):copyPathName
  (s):copySectionName
  (r):copyRectangle
  (0-9):copyToRegister

Document:
  (ESC):Main
  (NM):Main
  (NM):true
  (d):opeNMocument
  (t):fileTree
  (s):saveFile
  (d):deleteFile
  (e):exitFile
  (r):renameFile
  (p):changePath
  (P):changePermissions

Paste:
  (ESC):Main
  (NM):Main
  (NM):true
  (v):paste
  (n):pasteFileName
  (p):pastePathName
  (s):pasteSectionName
  (0-9):pasteRegister

Cut:
  (ESC):Main
  (NM):Main
  (NM):true
  (x):cut
  (l):cutLine

Save:
  (ESC):Main
  (NM):Main
  (NM):true
  (s):save
  (f):saveAllFiles
  (a):saveAs
  (e):exportAs

Help:
  (ESC):Main
  (NM):Main
  (NM):true
  (h):chatBotHelper
  (d):documentation
  (a):fuzzyDiscoverAllHelp
  (m):mythHelp
  (a):appHelp
  (k):keyboradHelp
  (f):functionHelp
  (v):variableHelp

Movement:
  //(exception to rule)
  (ESC):Main
  // Notice that per default the movement state changes to the movemnet state, this makes sense as movement is ofteh followed by more movement
  (NM):Movement
  (NM):true
  // avy like motion, shift numbers go to that line number
  (m):moveCursor
  // node, or line, or word depeNMing of what is selected
  (e):goToENM
  //node, or line, or word depeNMing of what is selected
  (b):goToBeginning
  //see what vim motions is interesing here, basically the movement part will be the vim part I beleive
  //except of course the basics, need to evaluate this
  //The C- here is not necessary
  (i):goUpLine
  (k):goLeftChar
  (j):goDownLine
  (l):goRightChar
  (I):goParent
  (K):goLeftSibling
  (K):goChild
  (L):goRightSibling

File:
  (ESC):Main
  (NM):Main
  (NM):true
  //like helm swoop
  (d):discoverInSection
  (D):discoverInProject
  (r):replaceInSection
  (R):replaceInProject
  (p):Disco
  // creates a menu with a file outlined (bullets in markdown etc)
  (m):menu

Buffer:
  (ESC):Main
  (NM):Buffer
  (NM):true
  (b):bufferSwitch
  (n):newBuffer
  (s):saveBufferToFile
  (v):layoutVerticalSplit
  (h):layoutHorizontalSplit
  (i):goLayoutUp
  (k):goLayoutDown
  (j):goLayoutLeft
  (l):goLayoutRight
  (e):enlargeLayout
  (r):reduceLayout
  (=):equalizeLayout
  (+):maximizeLayout
  (d):deleteSection
  (t):toogleNormalTabs
  (s):saveLayoutConfiguration
  (l):loadLayoutConfiguration
  (c):closeBuffer
  (I):enlargeLayoutVertical
  (K):reduceLayoutVertical
  (J):enlargeLayoutHorizontal
  (L):reduleLayoutHorizontal
  (a):fuzzyDiscoverAllHelp
  (m):mythHelp
  (a):appHelp
  (k):keyboradHelp
  (f):functionHelp
  (v):variableHelp

Movement:
  //(exception to rule)
  (ESC):Main
  // Notice that per default the movement state changes to the movemnet state, this makes sense as movement is ofteh followed by more movement
  (NM):Movement
  (NM):true
  // avy like motion, shift numbers go to that line number
  (m):moveCursor
  // node, or line, or word depeNMing of what is selected
  (e):goToENM
  //node, or line, or word depeNMing of what is selected
  (b):goToBeginning
  //see what vim motions is interesing here, basically the movement part will be the vim part I beleive
  //except of course the basics, need to evaluate this
  //The C- here is not necessary
  (i):goUpLine
  (k):goLeftChar
  (j):goDownLine
  (l):goRightChar
  (I):goParent
  (K):goLeftSibling
  (K):goChild
  (L):goRightSibling

File:
  (ESC):Main
  (NM):Main
  (NM):true
  //like helm swoop
  (d):discoverInSection
  (D):discoverInProject
  (r):replaceInSection
  (R):replaceInProject
  (p):Disco
  // creates a menu with a file outlined (bullets in markdown etc)
  (m):menu

Buffer:
  (ESC):Main
  (NM):Buffer
  (NM):true
  (b):bufferSwitch
  (n):newBuffer
  (s):saveBufferToFile
  (v):layoutVerticalSplit
  (h):layoutHorizontalSplit
  (i):goLayoutUp
  (k):goLayoutDown
  (j):goLayoutLeft
  (l):goLayoutRight
  (e):enlargeLayout
  (r):reduceLayout
  (=):equalizeLayout
  (+):maximizeLayout
  (d):deleteSection
  (t):toogleNormalTabs
  (s):saveLayoutConfiguration
  (l):loadLayoutConfiguration
  (c):closeBuffer
  (I):enlargeLayoutVertical
  (K):reduceLayoutVertical
  (J):enlargeLayoutHorizontal
  (L):reduleLayoutHorizontal
  (up):goLayoutUp
  (down):goLayoutDown
  (left):goLayoutLeft
  (right):goLayoutRight


Page:
  (ESC):Main
  (NM):Page
  (NM):true
  (n):newPage
  (p):changeNextPage
  (e):exitPage
  (e):enumeratePages
  (0-9):changePage
  (s):savePageConfiguration
  (l):loadPageConfiguration

Undo:
  (ESC):Main
  (NM):Undo
  (NM):true
  (z):undo
  (r):redo
  (v):undoSmallVizualizer


and then the apllication allowed (dynamically to implemnt those functions, And whn user was on the sequence of say Undo menu and clickd z, will do undo, ESC will go to the Main menu etc)

# Iteration two with tree sitter

Having queries for every test base, simmilar with the thing that associated tags with queries, with could also associate functions. 

And that means if the user was selecting a given node of the tree, it could also use those functionalites. 

(So test base everyhing all keybaord controlable really.)

(then the click will be just mapping to most internal node named node, but manaing cursor position on the clicked char potentially.)

# The idea is letting this go even a step firther and for isntances a file tree on a code editor can be basically disctibed also with a grammar:

file_line \newline 
file_line

and we could associate at file_line tags the funcitons {erase_file, new_file etc etc}.

We can also do the sampe for top options,

In concluion in practice the full App would be super text based (And the intereseting part would be to make it render in a beatutifull way.)

