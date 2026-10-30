/*
 * MyChess HTML viewer (spec M7-4, slice d; ADR 0016).
 *
 * Code written for MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md); no
 * file of the Scid vs PC is used. It runs from a single exported file: no
 * network, no eval, no markup built from game text. Everything that comes
 * from the database is put in the page with textContent; the board and the
 * controls are made with DOM calls. The games are read from the JSON block
 * "mychess-data" (see mychess-db::export::viewer for the model).
 */
(function () {
  'use strict';

  var SVG_NS = 'http://www.w3.org/2000/svg';
  var SQUARE = 45;
  var MARGIN = 20;
  var NAG_TEXT = {
    1: '!', 2: '?', 3: '!!', 4: '??', 5: '!?', 6: '?!', 7: '□', 10: '=',
    13: '∞', 14: '⩲', 15: '⩱', 16: '±', 17: '∓',
    18: '+-', 19: '-+', 22: '⨀', 23: '⨀', 32: '⟳', 33: '⟳',
    36: '→', 37: '→', 40: '↑', 41: '↑', 132: '⇆',
    133: '⇆', 140: '∆', 142: '⌓', 145: 'RR', 146: 'N'
  };

  var dataBlock = document.getElementById('mychess-data');
  var host = document.getElementById('viewer');
  if (!dataBlock || !host) {
    return;
  }
  var data;
  try {
    data = JSON.parse(dataBlock.textContent);
  } catch (error) {
    host.textContent = 'The games of this file could not be read.';
    return;
  }
  var games = Array.isArray(data.games) ? data.games : [];
  if (games.length === 0) {
    host.textContent = 'This file has no games.';
    return;
  }

  function el(tag, className, text) {
    var node = document.createElement(tag);
    if (className) {
      node.className = className;
    }
    if (text !== undefined && text !== null) {
      node.textContent = text;
    }
    return node;
  }

  function svgEl(tag, attributes) {
    var node = document.createElementNS(SVG_NS, tag);
    Object.keys(attributes || {}).forEach(function (name) {
      node.setAttribute(name, String(attributes[name]));
    });
    return node;
  }

  /* ---- state ---- */
  var gameIndex = 0;
  var cursor = 0;
  var flipped = false;
  var moveSpans = {};

  /* ---- board ---- */
  var board = svgEl('svg', {
    viewBox: '0 0 ' + (8 * SQUARE + 2 * MARGIN) + ' ' + (8 * SQUARE + 2 * MARGIN),
    'class': 'board',
    role: 'img',
    'aria-label': 'Chess board'
  });
  var squares = [];
  var highlights = [];
  var piecesLayer = svgEl('g', { 'class': 'pieces' });
  var fileLabels = [];
  var rankLabels = [];
  var r;
  var f;
  for (r = 0; r < 8; r += 1) {
    for (f = 0; f < 8; f += 1) {
      var rect = svgEl('rect', {
        width: SQUARE,
        height: SQUARE,
        'class': 'sq ' + ((r + f) % 2 === 0 ? 'l' : 'd')
      });
      board.appendChild(rect);
      squares.push(rect);
    }
  }
  for (r = 0; r < 64; r += 1) {
    var mark = svgEl('rect', { width: SQUARE, height: SQUARE, 'class': 'hl', visibility: 'hidden' });
    board.appendChild(mark);
    highlights.push(mark);
  }
  board.appendChild(piecesLayer);
  for (f = 0; f < 8; f += 1) {
    var fileLabel = svgEl('text', { 'class': 'co', 'text-anchor': 'middle' });
    board.appendChild(fileLabel);
    fileLabels.push(fileLabel);
    var rankLabel = svgEl('text', { 'class': 'co', 'text-anchor': 'middle' });
    board.appendChild(rankLabel);
    rankLabels.push(rankLabel);
  }

  function placement(fen) {
    var cells = [];
    var rows = String(fen).split(' ')[0].split('/');
    var i;
    var j;
    for (i = 0; i < 8; i += 1) {
      var row = rows[i] || '';
      for (j = 0; j < row.length; j += 1) {
        var c = row.charAt(j);
        if (c >= '1' && c <= '8') {
          var k;
          for (k = 0; k < Number(c); k += 1) {
            cells.push('');
          }
        } else {
          var lower = c.toLowerCase();
          cells.push((c === lower ? 'b' : 'w') + c.toUpperCase());
        }
      }
    }
    while (cells.length < 64) {
      cells.push('');
    }
    return cells.slice(0, 64);
  }

  function drawBoard(fen, previousFen) {
    var now = placement(fen);
    var before = previousFen === null ? now : placement(previousFen);
    while (piecesLayer.firstChild) {
      piecesLayer.removeChild(piecesLayer.firstChild);
    }
    var i;
    for (i = 0; i < 64; i += 1) {
      var rank = Math.floor(i / 8);
      var file = i % 8;
      var col = flipped ? 7 - file : file;
      var line = flipped ? 7 - rank : rank;
      var x = MARGIN + col * SQUARE;
      var y = MARGIN + line * SQUARE;
      var square = squares[i];
      square.setAttribute('x', String(x));
      square.setAttribute('y', String(y));
      var changed = now[i] !== before[i];
      var base = 'sq ' + ((rank + file) % 2 === 0 ? 'l' : 'd');
      square.setAttribute('class', base);
      highlights[i].setAttribute('x', String(x));
      highlights[i].setAttribute('y', String(y));
      highlights[i].setAttribute('visibility', changed ? 'visible' : 'hidden');
      if (now[i] !== '') {
        var piece = svgEl('use', {
          href: '#' + now[i],
          x: x,
          y: y,
          width: SQUARE,
          height: SQUARE
        });
        piecesLayer.appendChild(piece);
      }
    }
    for (i = 0; i < 8; i += 1) {
      // Label i sits in column i (files) or row i (ranks) of the screen.
      fileLabels[i].textContent = String.fromCharCode(97 + (flipped ? 7 - i : i));
      fileLabels[i].setAttribute('x', String(MARGIN + i * SQUARE + SQUARE / 2));
      fileLabels[i].setAttribute('y', String(MARGIN + 8 * SQUARE + 14));
      rankLabels[i].textContent = String(flipped ? i + 1 : 8 - i);
      rankLabels[i].setAttribute('x', String(MARGIN / 2));
      rankLabels[i].setAttribute('y', String(MARGIN + i * SQUARE + SQUARE / 2 + 4));
    }
  }

  /* ---- page ---- */
  var heading = el('div', 'heading');
  var details = el('div', 'details');
  var picker = el('select', 'picker');
  picker.setAttribute('aria-label', 'Game');
  games.forEach(function (game, index) {
    var label = String(game.n) + '. ' + (game.white || '?') + ' - ' + (game.black || '?');
    var option = el('option', null, label);
    option.value = String(index);
    picker.appendChild(option);
  });
  var movesBox = el('div', 'moves');
  movesBox.setAttribute('tabindex', '0');
  var controls = el('div', 'controls');

  function button(label, title, handler) {
    var b = el('button', null, label);
    b.type = 'button';
    b.title = title;
    b.setAttribute('aria-label', title);
    b.addEventListener('click', handler);
    return b;
  }

  var side = el('div', 'side');
  var layout = el('div', 'layout');
  var left = el('div', 'left');
  controls.appendChild(button('⇅', 'Flip the board (F)', function () {
    flipped = !flipped;
    show();
  }));
  controls.appendChild(button('|<', 'First position (Home)', function () { go('start'); }));
  controls.appendChild(button('<', 'Previous move (Left)', function () { go('prev'); }));
  controls.appendChild(button('>', 'Next move (Right)', function () { go('next'); }));
  controls.appendChild(button('>|', 'Last position (End)', function () { go('end'); }));
  controls.appendChild(button('<<', 'Previous game (Up)', function () { switchGame(gameIndex - 1); }));
  controls.appendChild(button('>>', 'Next game (Down)', function () { switchGame(gameIndex + 1); }));
  left.appendChild(board);
  left.appendChild(controls);
  side.appendChild(picker);
  side.appendChild(heading);
  side.appendChild(details);
  side.appendChild(movesBox);
  layout.appendChild(left);
  layout.appendChild(side);
  host.appendChild(layout);

  function current() {
    return games[gameIndex];
  }

  function nodes() {
    return current().nodes || [];
  }

  function fenParts(fen) {
    var parts = String(fen).split(' ');
    return { turn: parts[1] === 'b' ? 'b' : 'w', number: parts[5] || '1' };
  }

  function nagText(list) {
    var out = '';
    (list || []).forEach(function (nag) {
      out += Object.prototype.hasOwnProperty.call(NAG_TEXT, nag) ? NAG_TEXT[nag] : '$' + nag;
    });
    return out;
  }

  function addMove(index, parentIndex, forceNumber, box) {
    var all = nodes();
    var node = all[index];
    var parent = all[parentIndex];
    var parts = fenParts(parent.fen);
    var prefix = '';
    if (parts.turn === 'w') {
      prefix = parts.number + '.';
    } else if (forceNumber) {
      prefix = parts.number + '...';
    }
    var span = el('span', 'mv', (prefix === '' ? '' : prefix + ' ') + node.san + nagText(node.nags));
    span.setAttribute('data-node', String(index));
    span.setAttribute('role', 'button');
    span.setAttribute('tabindex', '0');
    span.addEventListener('click', function () {
      cursor = index;
      show();
    });
    span.addEventListener('keydown', function (event) {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        event.stopPropagation();
        cursor = index;
        show();
      }
    });
    moveSpans[index] = span;
    box.appendChild(span);
    box.appendChild(document.createTextNode(' '));
    if (node.comment) {
      box.appendChild(el('span', 'c', node.comment));
      box.appendChild(document.createTextNode(' '));
    }
  }

  function run(parentIndex, childIndex, box) {
    var all = nodes();
    var parent = parentIndex;
    var child = childIndex;
    var interrupted = true;
    var guard = 0;
    while (child >= 0 && guard < 100000) {
      guard += 1;
      addMove(child, parent, interrupted, box);
      interrupted = Boolean(all[child].comment);
      if (all[parent].next === child) {
        var alternatives = all[parent].variations || [];
        alternatives.forEach(function (variation) {
          var inner = el('div', 'var');
          run(parent, variation, inner);
          box.appendChild(inner);
          interrupted = true;
        });
      }
      parent = child;
      child = all[child].next;
    }
  }

  function buildMoves() {
    moveSpans = {};
    while (movesBox.firstChild) {
      movesBox.removeChild(movesBox.firstChild);
    }
    var all = nodes();
    if (all.length === 0) {
      return;
    }
    if (all[0].comment) {
      movesBox.appendChild(el('span', 'c', all[0].comment));
      movesBox.appendChild(document.createTextNode(' '));
    }
    run(0, all[0].next, movesBox);
    movesBox.appendChild(el('span', 'result', current().result || '*'));
  }

  function buildHeading() {
    var game = current();
    var white = (game.white || '?') + (game.white_elo ? ' (' + game.white_elo + ')' : '');
    var black = (game.black || '?') + (game.black_elo ? ' (' + game.black_elo + ')' : '');
    heading.textContent = white + ' - ' + black;
    var bits = [];
    if (game.event) { bits.push(game.event + (game.round ? ' (' + game.round + ')' : '')); }
    if (game.site) { bits.push(game.site); }
    if (game.date) { bits.push(game.date); }
    if (game.result) { bits.push(game.result); }
    if (game.eco) { bits.push(game.eco); }
    details.textContent = bits.join('  ·  ');
  }

  function show() {
    var all = nodes();
    if (cursor < 0 || cursor >= all.length) {
      cursor = 0;
    }
    var node = all[cursor];
    var previous = node.prev >= 0 && all[node.prev] ? all[node.prev].fen : null;
    drawBoard(node.fen, previous);
    Object.keys(moveSpans).forEach(function (key) {
      moveSpans[key].className = 'mv';
    });
    if (moveSpans[cursor]) {
      moveSpans[cursor].className = 'mv sel';
      if (typeof moveSpans[cursor].scrollIntoView === 'function') {
        moveSpans[cursor].scrollIntoView({ block: 'nearest' });
      }
    }
  }

  function go(where) {
    var all = nodes();
    if (where === 'start') {
      cursor = 0;
    } else if (where === 'prev') {
      if (all[cursor].prev >= 0) { cursor = all[cursor].prev; }
    } else if (where === 'next') {
      if (all[cursor].next >= 0) { cursor = all[cursor].next; }
    } else {
      var guard = 0;
      while (all[cursor].next >= 0 && guard < 100000) {
        guard += 1;
        cursor = all[cursor].next;
      }
    }
    show();
  }

  function switchGame(index) {
    if (index < 0 || index >= games.length) {
      return;
    }
    gameIndex = index;
    cursor = 0;
    picker.value = String(index);
    buildHeading();
    buildMoves();
    show();
  }

  picker.addEventListener('change', function () {
    switchGame(Number(picker.value));
  });

  document.addEventListener('keydown', function (event) {
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    var target = event.target;
    if (target && target.tagName === 'SELECT') {
      return;
    }
    // In the move list the vertical keys scroll the list natively: they must
    // neither switch the game (the focus would be lost) nor be cancelled.
    var inMoves = target && target.closest && target.closest('.moves');
    if (inMoves && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      return;
    }
    var handled = true;
    switch (event.key) {
      case 'ArrowLeft': go('prev'); break;
      case 'ArrowRight': go('next'); break;
      case 'Home': go('start'); break;
      case 'End': go('end'); break;
      case 'ArrowUp': switchGame(gameIndex - 1); break;
      case 'ArrowDown': switchGame(gameIndex + 1); break;
      case 'f':
      case 'F':
        flipped = !flipped;
        show();
        break;
      default: handled = false;
    }
    // In the move list Home/End keep their native scrolling; the board still
    // follows.
    var scrolls = event.key === 'Home' || event.key === 'End';
    if (handled && !(inMoves && scrolls)) {
      event.preventDefault();
    }
  });

  switchGame(0);
}());
