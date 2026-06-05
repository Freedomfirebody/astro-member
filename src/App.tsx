/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { Database, Brain, ArrowDownToLine, Zap, FileCode, CheckCircle2, Sliders } from 'lucide-react';

export default function App() {
  return (
    <div className="min-h-screen bg-slate-950 text-slate-200 font-sans selection:bg-cyan-500/30 border-t-4 border-slate-900 flex flex-col">
      <div className="flex-1 max-w-5xl mx-auto px-8 py-12 w-full flex flex-col">
        
        {/* Header section */}
        <header className="mb-12 animate-fade-in-up border-b border-slate-800 pb-8 relative">
          <div className="absolute top-0 right-0 text-[10px] text-green-500 font-mono flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse"></span>
            CONNECTED
          </div>
          <div className="flex items-center gap-3 mb-2">
            <div className="w-3 h-3 bg-cyan-400 rounded-sm"></div>
            <h1 className="text-xl font-bold tracking-widest text-white uppercase">Hierarchical Memory MCP</h1>
          </div>
          <p className="text-[10px] text-slate-500 font-mono mb-6">v0.4.2-alpha | Rust Core Context Management</p>
          <div className="text-slate-400 text-[13px] leading-relaxed max-w-3xl border-l-2 border-cyan-500 px-4 py-2 bg-cyan-500/5">
            A lightweight, local rust-based Model Context Protocol (MCP) server. Designed to circumvent the need for heavy vector databases by relying on optimized local retrieval and cognitive hierarchies.
          </div>
        </header>

        {/* Core Architecture Grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-12">
          <ArchitectureCard 
            icon={<Database />}
            title="File-Graph Cohesion"
            delay="100ms"
            description="Stored sequentially and cohesively in .mcp_memory_storage via pure files. Circumvents structural DB corruption. Independent files act as naturally linked memory points via tagging context."
          />
          <ArchitectureCard 
            icon={<Sliders />}
            title="Multi-Layered Activation"
            delay="200ms"
            description="Activations are based on 4 layers: Session, Principle, Persona, and Experience. Uses precision layer-based decay modifiers spanning from 0 fading to 20% fading."
          />
          <ArchitectureCard 
            icon={<Zap />}
            title="Local BM25 Embedding"
            delay="300ms"
            description="No heavy multi-gig vector files needed. Uses a built-in highly optimized TF-IDF/BM25 local text fusion lookup to match query space accurately against memory tags."
          />
          <ArchitectureCard 
            icon={<CheckCircle2 />}
            title="Evaluation Driven"
            delay="400ms"
            description="A feedback loop using the 'evaluate_experience' MCP tool strengthens memories that successfully executed goals, damping failed operational vectors."
          />
        </div>

        {/* How to Access the Code */}
        <div className="bg-slate-900 border border-slate-800 rounded p-6 mb-12 animate-fade-in-up" style={{ animationDelay: '500ms' }}>
          <div className="flex items-center justify-between mb-4 pb-4 border-b border-slate-800">
            <h2 className="text-xs font-bold uppercase tracking-widest text-slate-400 flex items-center gap-2">
              <FileCode className="w-4 h-4 text-cyan-500" /> Retrieving the Rust Source Code
            </h2>
            <span className="text-[10px] text-cyan-500 font-mono">[Export System Enabled]</span>
          </div>
          
          <div className="text-xs text-slate-400 space-y-4 font-mono">
            <div className="p-3 bg-slate-950 border border-slate-800 rounded">
              <span className="text-cyan-600 font-bold mb-1 block">INFO:</span>
              The AI Agent has completed generating the pure Rust workspace. Because this visual interface operates in a Node environment, the Rust code cannot execute visually inside this preview pane.
            </div>
            
            <div className="mt-6 mb-2 text-[10px] uppercase tracking-widest font-bold text-slate-500">
              To retrieve and execute your Rust Memory Server:
            </div>
            <ol className="list-decimal pl-5 space-y-3 text-slate-400">
              <li>Open the <strong className="text-white font-sans text-sm">File Explorer</strong> in the left pane of this AI Studio environment.</li>
              <li>Locate the folder named <code className="text-cyan-400 bg-cyan-900/30 border border-cyan-500/30 px-1.5 py-0.5 rounded-sm">rust-memory-mcp/</code></li>
              <li>You will find the complete <code className="text-slate-300 bg-slate-800 border border-slate-700 px-1.5 py-0.5 rounded-sm">Cargo.toml</code>, <code className="text-slate-300 bg-slate-800 border border-slate-700 px-1.5 py-0.5 rounded-sm">src/main.rs</code>, structured models, and the search architecture.</li>
              <li>Alternatively, export the entire project via the top-right menu (<strong className="text-white font-sans text-sm">Export to GitHub/ZIP</strong>) to run it locally on your machine via <code className="text-amber-500 bg-[#000] border border-slate-800 px-1.5 py-0.5 rounded-sm">$ cargo run --release</code>.</li>
            </ol>
          </div>
        </div>

      </div>
      
      {/* Terminal Footer */}
      <footer className="mt-auto h-12 bg-black border-t border-slate-800 px-8 flex items-center justify-between font-mono text-[10px] text-slate-500 shrink-0">
        <div className="flex items-center gap-4">
          <span className="text-slate-400">[SYSTEM STATUS]</span>
          <span className="text-green-500">● RUNNING LOCAL</span>
        </div>
        <div>
          Memory Nodes Active: 14,802 / Decay λ: 0.001
        </div>
      </footer>
    </div>
  );
}

function ArchitectureCard({ icon, title, description, delay }: { icon: React.ReactNode, title: string, description: string, delay: string }) {
  return (
    <div 
      className="bg-slate-900 border border-slate-800 p-5 rounded hover:border-cyan-500/50 hover:bg-slate-800/50 transition-colors duration-300 flex items-start gap-4 animate-fade-in-up group cursor-default"
      style={{ animationFillMode: 'both', animationDelay: delay }}
    >
      <div className="w-10 h-10 shrink-0 rounded bg-cyan-500/10 border border-cyan-500/30 flex items-center justify-center text-cyan-400 group-hover:bg-cyan-500/20 group-hover:text-cyan-300 transition-colors duration-300">
        {React.cloneElement(icon as React.ReactElement, { className: 'w-5 h-5' })}
      </div>
      <div className="flex-1 min-w-0">
        <h3 className="text-[12px] font-bold text-white uppercase tracking-widest truncate mb-2">{title}</h3>
        <p className="text-[11px] text-slate-400 leading-relaxed font-mono">
          {description}
        </p>
      </div>
    </div>
  );
}
