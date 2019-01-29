// Copyright 2016-present Facebook. All Rights Reserved.
//
// node_test.c: unit tests for the node.c
//
// no-check-code

#include "node.h"
#include "tests.h"

#define ALLOC_NODE_STR(name, max_children) \
  alloc_node(name, strlen(name), max_children)
#define GET_CHILD_BY_NAME_STR(node, name) \
  get_child_by_name(node, name, strlen(name))

/**
 * Add a child and ensure that it can be found.
 */
void test_simple_parent_child() {
  node_t* parent = ALLOC_NODE_STR("parent", 1);
  node_t* child = ALLOC_NODE_STR("child", 0);
  parent->in_use = true;
  parent->type = TYPE_IMPLICIT;
  child->in_use = true;
  child->type = TYPE_LEAF;

  node_add_child_result_t result = add_child(parent, child);
  ASSERT(result == ADD_CHILD_OK);

  node_t* lookup_child = GET_CHILD_BY_NAME_STR(parent, "child");
  ASSERT(lookup_child == child);
}

/**
 * Ensure that our size calculations are reasonable accurate by allocating a
 * bunch of differently sized parents and adding a child.
 */
void test_space() {
  for (uint16_t name_sz = 1; name_sz <= 8; name_sz++) {
    node_t* parent = alloc_node("abcdefgh", name_sz, 1);
    node_t* child = ALLOC_NODE_STR("child", 0);
    parent->in_use = true;
    parent->type = TYPE_IMPLICIT;
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    node_t* lookup_child = GET_CHILD_BY_NAME_STR(parent, "child");
    ASSERT(lookup_child == child);
  }
}

/**
 * Try to add a child to a node that does not have enough space.
 */
void test_insufficient_space() {
  node_t* parent = ALLOC_NODE_STR("parent", 1);
  node_t* child1 = ALLOC_NODE_STR("child1", 0);
  node_t* child2 = ALLOC_NODE_STR("child2", 0);
  parent->in_use = true;
  parent->type = TYPE_IMPLICIT;
  child1->in_use = true;
  child1->type = TYPE_LEAF;
  child2->in_use = true;
  child2->type = TYPE_LEAF;

  node_add_child_result_t result = add_child(parent, child1);
  ASSERT(result == ADD_CHILD_OK);
  result = add_child(parent, child2);
  ASSERT(result == NEEDS_LARGER_NODE);

  node_t* lookup_child = GET_CHILD_BY_NAME_STR(parent, "child1");
  ASSERT(lookup_child == child1);
  lookup_child = GET_CHILD_BY_NAME_STR(parent, "child2");
  ASSERT(lookup_child == NULL);
}

/**
 * Call `add_child` with a bunch of different arguments and verify the results
 * are reasonable.
 */
typedef struct {
  bool parent_in_use;
  int parent_type;
  bool child_in_use;
  int child_type;
  node_add_child_result_t expected_result;
} parent_child_test_cases_t;

void test_add_child_combinations() {
  parent_child_test_cases_t cases[] = {
      // parent or child not in use.
      {false, TYPE_IMPLICIT, true, TYPE_LEAF, ADD_CHILD_ILLEGAL_PARENT},
      {true, TYPE_IMPLICIT, false, TYPE_LEAF, ADD_CHILD_ILLEGAL_CHILD},

      // parent type invalid.
      {true, TYPE_LEAF, true, TYPE_LEAF, ADD_CHILD_ILLEGAL_PARENT},

      // child type invalid.
      {true, TYPE_IMPLICIT, false, TYPE_UNDEFINED, ADD_CHILD_ILLEGAL_CHILD},

      // some good outcomes.
      {true, TYPE_IMPLICIT, true, TYPE_LEAF, ADD_CHILD_OK},
      {true, TYPE_IMPLICIT, true, TYPE_IMPLICIT, ADD_CHILD_OK},
  };

  for (int ix = 0; ix < sizeof(cases) / sizeof(parent_child_test_cases_t);
       ix++) {
    node_t* parent;
    node_t* child;

    parent = ALLOC_NODE_STR("parent", 1);
    child = ALLOC_NODE_STR("child", 0);

    parent->in_use = cases[ix].parent_in_use;
    parent->type = cases[ix].parent_type;
    child->in_use = cases[ix].child_in_use;
    child->type = cases[ix].child_type;
    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == cases[ix].expected_result);
  }
}

/**
 * Insert children in lexicographical order.  Ensure that we can find them.
 *
 * requirement: strlen(TEST_MANY_CHILDREN_NAME_STR) >=
 *              TEST_MANY_CHILDREN_CHILD_COUNT
 */
#define TEST_MANY_CHILDREN_NAME_STR "abcdefgh"
#define TEST_MANY_CHILDREN_COUNT 8

void test_many_children() {
  node_t* parent = ALLOC_NODE_STR("parent", TEST_MANY_CHILDREN_COUNT);
  node_t* children[TEST_MANY_CHILDREN_COUNT]; // this should be ordered as we
                                              // expect to find them in the
                                              // parent's list of children.
  for (uint16_t name_sz = 1; name_sz <= TEST_MANY_CHILDREN_COUNT; name_sz++) {
    node_t* child = alloc_node(TEST_MANY_CHILDREN_NAME_STR, name_sz, 0);
    parent->in_use = true;
    parent->type = TYPE_IMPLICIT;
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    children[name_sz - 1] = child;
  }

  for (uint16_t name_sz = 1; name_sz <= TEST_MANY_CHILDREN_COUNT; name_sz++) {
    node_t* result =
        get_child_by_name(parent, TEST_MANY_CHILDREN_NAME_STR, name_sz);
    ASSERT(result == children[name_sz - 1]);
  }
}

/**
 * Insert children in reverse lexicographical order.  Ensure that we can find
 * them.
 *
 * requirement: strlen(TEST_MANY_CHILDREN_NAME_STR) >=
 *              TEST_MANY_CHILDREN_CHILD_COUNT
 */
void test_many_children_reverse() {
  node_t* parent = ALLOC_NODE_STR("parent", TEST_MANY_CHILDREN_COUNT);
  node_t* children[TEST_MANY_CHILDREN_COUNT]; // this should be ordered as we
                                              // expect to find them in the
                                              // parent's list of children.
  for (uint16_t name_sz = TEST_MANY_CHILDREN_COUNT; name_sz > 0; name_sz--) {
    node_t* child = alloc_node(TEST_MANY_CHILDREN_NAME_STR, name_sz, 0);
    parent->in_use = true;
    parent->type = TYPE_IMPLICIT;
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    children[name_sz - 1] = child;
  }

  for (uint16_t name_sz = 1; name_sz <= TEST_MANY_CHILDREN_COUNT; name_sz++) {
    node_t* result =
        get_child_by_name(parent, TEST_MANY_CHILDREN_NAME_STR, name_sz);
    ASSERT(result == children[name_sz - 1]);
  }
}

/**
 * Create a node with many children.  Clone the node.  Ensure we can locate all
 * of the children.
 *
 * requirement: strlen(TEST_CLONE_NAME_STR) >=
 *              TEST_CLONE_COUNT
 */
#define TEST_CLONE_NAME_STR "abcdefgh"
#define TEST_CLONE_COUNT 8

void test_clone() {
  node_t* parent = ALLOC_NODE_STR("parent", TEST_CLONE_COUNT);
  parent->in_use = true;
  parent->type = TYPE_IMPLICIT;
  memset(parent->checksum, 0x2e, SHA1_BYTES);
  parent->checksum_valid = true;
  parent->checksum_sz = SHA1_BYTES;
  parent->flags = 0x3e;

  node_t* children[TEST_CLONE_COUNT]; // this should be ordered as we
                                      // expect to find them in the
                                      // parent's list of children.
  for (uint16_t name_sz = 1; name_sz <= TEST_CLONE_COUNT; name_sz++) {
    node_t* child = alloc_node(TEST_CLONE_NAME_STR, name_sz, 0);
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    children[name_sz - 1] = child;
  }

  node_t* clone = clone_node(parent);

  for (uint16_t name_sz = 1; name_sz <= TEST_CLONE_COUNT; name_sz++) {
    node_t* result = get_child_by_name(clone, TEST_CLONE_NAME_STR, name_sz);
    ASSERT(result == children[name_sz - 1]);
  }

  ASSERT(clone->checksum_sz == SHA1_BYTES);
  for (uint8_t ix = 0; ix < SHA1_BYTES; ix++) {
    ASSERT(clone->checksum[ix] == 0x2e);
  }
  ASSERT(clone->flags == 0x3e);

  ASSERT(max_children(clone) > max_children(parent));
}

/**
 * Create a node with many children.  Remove them in a pseudorandom fashion.
 * Ensure that the remaining children can be correctly found.
 *
 * requirement: strlen(TEST_REMOVE_CHILD_NAME_STR) >=
 *              TEST_REMOVE_CHILD_COUNT
 */
#define TEST_REMOVE_CHILD_NAME_STR "1234ffgg"
#define TEST_REMOVE_CHILD_COUNT 8

void test_remove_child() {
  node_t* parent = ALLOC_NODE_STR("parent", TEST_REMOVE_CHILD_COUNT);
  node_t* children[TEST_REMOVE_CHILD_COUNT]; // this should be ordered as we
                                             // expect to find them in the
                                             // parent's list of children.
  bool valid[TEST_REMOVE_CHILD_COUNT];
  for (uint16_t name_sz = 1; name_sz <= TEST_REMOVE_CHILD_COUNT; name_sz++) {
    node_t* child = alloc_node(TEST_REMOVE_CHILD_NAME_STR, name_sz, 0);
    parent->in_use = true;
    parent->type = TYPE_IMPLICIT;
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    children[name_sz - 1] = child;
    valid[name_sz - 1] = true;
  }

  for (uint16_t ix = 0; ix < TEST_REMOVE_CHILD_COUNT; ix++) {
    uint16_t victim_index = 0;
    for (uint16_t jx = 0; jx < TEST_REMOVE_CHILD_COUNT + 1; jx++) {
      do {
        victim_index = (victim_index + 1) % TEST_REMOVE_CHILD_COUNT;
      } while (valid[victim_index] == false);
    }

    // ok, we found our victim.  remove it.
    node_search_children_result_t search_result =
        search_children(parent, TEST_REMOVE_CHILD_NAME_STR, victim_index + 1);

    ASSERT(search_result.child == children[victim_index]);
    valid[victim_index] = false;

    ASSERT(remove_child(parent, search_result.child_num) == REMOVE_CHILD_OK);

    // go through the items that should still be children, and make sure they're
    // still reachable.
    for (uint16_t name_sz = 1; name_sz <= TEST_REMOVE_CHILD_COUNT; name_sz++) {
      node_t* child =
          get_child_by_name(parent, TEST_REMOVE_CHILD_NAME_STR, name_sz);
      if (valid[name_sz - 1]) {
        ASSERT(child != NULL);
      } else {
        ASSERT(child == NULL);
      }
    }
  }
}

/**
 * Create a node and add many children.  Enlarge one of the children.
 *
 * requirement: strlen(TEST_ENLARGE_CHILD_CAPACITY_NAME_STR) >=
 *              TEST_ENLARGE_CHILD_CAPACITY_COUNT
 */
#define TEST_ENLARGE_CHILD_CAPACITY_NAME_STR "abcdefgh"
#define TEST_ENLARGE_CHILD_CAPACITY_COUNT 8

void test_enlarge_child_capacity() {
  node_t* parent = ALLOC_NODE_STR("parent", TEST_MANY_CHILDREN_COUNT);
  node_t* children[TEST_MANY_CHILDREN_COUNT]; // this should be ordered as we
                                              // expect to find them in the
                                              // parent's list of children.
  for (uint16_t name_sz = 1; name_sz <= TEST_MANY_CHILDREN_COUNT; name_sz++) {
    node_t* child =
        alloc_node(TEST_ENLARGE_CHILD_CAPACITY_NAME_STR, name_sz, 0);
    parent->in_use = true;
    parent->type = TYPE_IMPLICIT;
    child->in_use = true;
    child->type = TYPE_LEAF;

    node_add_child_result_t result = add_child(parent, child);
    ASSERT(result == ADD_CHILD_OK);

    children[name_sz - 1] = child;
  }

  node_enlarge_child_capacity_result_t enlarge_child_capacity_result =
      enlarge_child_capacity(parent, 0);
  ASSERT(enlarge_child_capacity_result.code == ENLARGE_OK);
  ASSERT(enlarge_child_capacity_result.old_child == children[0]);

  node_t* enlarged = get_child_by_index(parent, 0);
  ASSERT(max_children(enlarged) > 0);
  ASSERT(
      name_compare(
          enlarged->name,
          enlarged->name_sz,
          enlarge_child_capacity_result.old_child) == 0);
}

int main(int argc, char* argv[]) {
  test_simple_parent_child();
  test_space();
  test_insufficient_space();
  test_add_child_combinations();
  test_many_children();
  test_many_children_reverse();
  test_clone();
  test_remove_child();
  test_enlarge_child_capacity();

  return 0;
}
