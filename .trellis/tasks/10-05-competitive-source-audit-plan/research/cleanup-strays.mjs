import fs from 'node:fs';
const targets = ['C:/AgentSessions/1e11', 'C:/AgentSessions/1e14', 'C:/AgentSessions/='];
for (const t of targets) {
  if (fs.existsSync(t)) {
    const st = fs.statSync(t);
    if (st.isFile() && st.size === 0) { fs.unlinkSync(t); console.log('removed:', t); }
    else console.log('SKIP not empty file:', t, st.size, st.isDirectory());
  } else console.log('absent:', t);
}
const leftover = targets.filter(t => fs.existsSync(t));
console.log('leftover:', JSON.stringify(leftover));
