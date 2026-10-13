import java.util.*;
/** Semantic closure verification. Synthetic timing is not a phone measurement. */
public final class GraphAlgorithmTest {
  static <T> void candidate(Map<T,Set<T>> reads,Map<T,Set<T>> transitive) {
    try {var method=Class.forName("net.wachiland.android.compat.ModuleGraphClosure").getDeclaredMethod("compute",Map.class,Map.class);method.setAccessible(true);method.invoke(null,reads,transitive);}catch(Exception e){throw new RuntimeException(e);}
  }
  static <T> void stock(Map<T,Set<T>> reads,Map<T,Set<T>> transitive) {
    boolean changed;List<T> additions=new ArrayList<>();
    do {changed=false;for(Set<T> row:reads.values()) {
      for(T node:row){Set<T> requires=transitive.get(node);if(requires!=null)for(T next:requires)if(!row.contains(next))additions.add(next);}
      if(!additions.isEmpty()){row.addAll(additions);additions.clear();changed=true;}
    }}while(changed);
  }
  static Map<Integer,Set<Integer>> copy(Map<Integer,Set<Integer>> map){Map<Integer,Set<Integer>> result=new HashMap<>();map.forEach((k,v)->result.put(k,new HashSet<>(v)));return result;}
  static Map<Integer,Set<Integer>> graph(Random r,int n,double density,boolean partial){Map<Integer,Set<Integer>> g=new HashMap<>();for(int i=0;i<n;i++)if(!partial||r.nextBoolean()){Set<Integer> s=new HashSet<>();for(int j=0;j<n;j++)if(r.nextDouble()<density)s.add(j);g.put(i,s);}return g;}
  public static void main(String[] args){Random r=new Random(20261013);int cases=0;
    for(int n:new int[]{0,1,2,8,31,64,129})for(double density:new double[]{0,.02,.15,.5,1})for(int repeat=0;repeat<8;repeat++) {
      var initial=graph(r,n,density,true);var transitive=graph(r,n,density,true);var a=copy(initial);var b=copy(initial);stock(a,transitive);candidate(b,transitive);if(!a.equals(b))throw new AssertionError("Mismatch n="+n+" density="+density);cases++;
    }
    var initial=graph(r,500,1,false);var transitive=graph(r,500,1,false);
    long t=System.nanoTime();var a=copy(initial);stock(a,transitive);long stockTime=System.nanoTime()-t;
    t=System.nanoTime();var b=copy(initial);candidate(b,transitive);long candidateTime=System.nanoTime()-t;
    if(!a.equals(b))throw new AssertionError("dense mismatch");
    System.out.println("equivalence_cases="+cases+" dense_nodes=500 desktop_stock_ms="+stockTime/1e6+" desktop_candidate_ms="+candidateTime/1e6+"; includes graph copy, NOT Android startup measurement");
  }
}
