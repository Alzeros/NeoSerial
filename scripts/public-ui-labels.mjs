import {readFileSync,readdirSync} from 'node:fs';
import path from 'node:path';
import {parse} from 'svelte/compiler';

/** Only literal template text, never script values, attributes, expressions or input values. */
export function publicUiLabels(directory) {
  const labels=new Set();
  function walk(node) {
    if(!node||typeof node!=='object') return;
    if(node.type==='Attribute'||node.type==='SpreadAttribute'||node.type?.endsWith('Directive')) return;
    if(node.type==='Text'&&typeof node.data==='string') {
      const text=node.data.replace(/\s+/g,' ').trim();
      if(text&&text.length<=300) labels.add(text);
    }
    for(const value of Object.values(node)) if(value&&typeof value==='object') {
      if(Array.isArray(value)) value.forEach(walk); else walk(value);
    }
  }
  function visit(dir) {
    for(const entry of readdirSync(dir,{withFileTypes:true})) {
      const file=path.join(dir,entry.name);
      if(entry.isDirectory()) visit(file);
      else if(entry.name.endsWith('.svelte')) walk(parse(readFileSync(file,'utf8'),{modern:true}).fragment);
    }
  }
  visit(directory); return [...labels].sort();
}
