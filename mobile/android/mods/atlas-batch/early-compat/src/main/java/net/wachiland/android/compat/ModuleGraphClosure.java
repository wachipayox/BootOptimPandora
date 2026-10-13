package net.wachiland.android.compat;
import java.util.*;
/** Startup-only closure of the exact existing graph; no dependency or validation skipped. */
public final class ModuleGraphClosure {
    private static final ThreadLocal<Long> controlStart=new ThreadLocal<>();
    private ModuleGraphClosure(){}
    public static boolean tryCandidate(Map<Object,Set<Object>> reads,Map<Object,Set<Object>> transitive) {
        String route=System.getProperty("wachiland.android.startup.moduleGraph","");
        if(route.equals("control")){controlStart.set(System.nanoTime());return false;}
        if(!route.equals("candidate") || reads.getClass()!=HashMap.class || transitive.getClass()!=HashMap.class)return false;
        Set<Set<Object>> identities=Collections.newSetFromMap(new IdentityHashMap<>());
        Set<Object> nodes=new HashSet<>();
        for(var map:List.of(reads,transitive))for(var entry:map.entrySet()) {
            if(entry.getKey()==null || !entry.getKey().getClass().getName().equals("java.lang.module.ResolvedModule"))return false;
            nodes.add(entry.getKey());Set<Object> row=entry.getValue();
            if(row==null || row.getClass()!=HashSet.class)return false;
            if(map==reads && !identities.add(row))return false;
            nodes.addAll(row);if(nodes.size()>2048)return false;
        }
        long start=System.nanoTime();compute(reads,transitive);
        System.out.println("[Wachiland module graph] candidate closure_ms="+(System.nanoTime()-start)/1e6+" modules="+reads.size()+" graph_nodes="+nodes.size());
        return true;
    }
    public static void finishControl(){Long start=controlStart.get();if(start!=null){controlStart.remove();System.out.println("[Wachiland module graph] stock closure_ms="+(System.nanoTime()-start)/1e6);}}
  static <T> void compute(Map<T,Set<T>> reads,Map<T,Set<T>> transitive) {
    Map<T,Integer> ids=new HashMap<>();List<T> nodes=new ArrayList<>();
    for(Map<T,Set<T>> map:List.of(reads,transitive))for(var entry:map.entrySet()) {
      if(!ids.containsKey(entry.getKey())){ids.put(entry.getKey(),nodes.size());nodes.add(entry.getKey());}
      if(entry.getValue()!=null)for(T node:entry.getValue())if(!ids.containsKey(node)){ids.put(node,nodes.size());nodes.add(node);}
    }
    int size=nodes.size();BitSet[] closure=new BitSet[size];
    for(int i=0;i<size;i++){closure[i]=new BitSet(size);Set<T> edges=transitive.get(nodes.get(i));if(edges!=null)for(T node:edges)closure[i].set(ids.get(node));}
    for(int k=0;k<size;k++)for(int i=0;i<size;i++)if(closure[i].get(k))closure[i].or(closure[k]);
    for(Set<T> edges:reads.values()) {
      BitSet expanded=new BitSet(size);
      for(T node:edges){int id=ids.get(node);expanded.set(id);expanded.or(closure[id]);}
      for(int id=expanded.nextSetBit(0);id>=0;id=expanded.nextSetBit(id+1))edges.add(nodes.get(id));
    }
  }
}
