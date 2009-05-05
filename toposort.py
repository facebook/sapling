''
"""
   Tarjan's algorithm and topological sorting implementation in Python
   by Paul Harrison
   Public domain, do with it as you will
"""
class TopoSort(object):

    def __init__(self, commitdict):
        self._sorted = self.robust_topological_sort(commitdict)
        self._shas = []
        for level in self._sorted:
            for sha in level:
                self._shas.append(sha)

    def items(self):
        self._shas.reverse()
        return self._shas

    def strongly_connected_components(self, graph):
        """ Find the strongly connected components in a graph using
            Tarjan's algorithm.

            graph should be a dictionary mapping node names to
            lists of successor nodes.
            """

        result = [ ]
        stack = [ ]
        low = { }

        def visit(node):
            if node in low: return

            num = len(low)
            low[node] = num
            stack_pos = len(stack)
            stack.append(node)

            for successor in graph[node].parents:
                visit(successor)
                low[node] = min(low[node], low[successor])

            if num == low[node]:
                component = tuple(stack[stack_pos:])
                del stack[stack_pos:]
                result.append(component)
                for item in component:
                    low[item] = len(graph)

        for node in graph:
            visit(node)

        return result


    def topological_sort(self, graph):
        count = { }
        for node in graph:
            count[node] = 0
        for node in graph:
            for successor in graph[node]:
                count[successor] += 1

        ready = [ node for node in graph if count[node] == 0 ]

        result = [ ]
        while ready:
            node = ready.pop(-1)
            result.append(node)

            for successor in graph[node]:
                count[successor] -= 1
                if count[successor] == 0:
                    ready.append(successor)

        return result


    def robust_topological_sort(self, graph):
        """ First identify strongly connected components,
            then perform a topological sort on these components. """

        components = self.strongly_connected_components(graph)

        node_component = { }
        for component in components:
            for node in component:
                node_component[node] = component

        component_graph = { }
        for component in components:
            component_graph[component] = [ ]

        for node in graph:
            node_c = node_component[node]
            for successor in graph[node].parents:
                successor_c = node_component[successor]
                if node_c != successor_c:
                    component_graph[node_c].append(successor_c)

        return self.topological_sort(component_graph)
