#[allow(unused_variables)]

extern crate petgraph;
#[macro_use]
extern crate failure;

use std::collections::{HashMap, HashSet, BTreeMap};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::Direction;
use failure::ResultExt;

pub type CellID = NodeIndex;
pub type CallbackID = u32;

#[derive(Debug)]
pub struct Reactor<'a, T> {
    /* A directed graph where each node is a cell pointing towards its dependencies

       Input cells are leaves as they don't have dependencies.
       Computed cells points towards other cells.

       The edge weight represents the index of a dependency cell in the argument list of the compute function.
       (Needed because the insertion order of the edges isn't preserved when walking over neighbor nodes)
    */
    pub dep_graph: Graph<Cell<'a, T>, usize>,
    // an increasing counter of used callback IDs.
    cur_callback_id: CallbackID,
}

#[derive(Debug)]
pub enum Cell<'a, T> {
    Input(InputCell<T>),
    Computed(ComputedCell<'a, T>)
}

#[derive(Debug)]
pub struct InputCell<T> {
    value: T
}

pub struct ComputedCell<'a, T> {
    value: T,
    compute_func: Box<Fn(&[T]) -> T>,
    callbacks: HashMap<CallbackID, Box<FnMut(T) -> () + 'a>>,
}
impl <'a, T: std::fmt::Debug> ::std::fmt::Debug for ComputedCell<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Computed {{ value: {:?}, callbacks_len: {} }}", self.value, self.callbacks.len())
    }
}


#[derive(Debug, Fail)]
pub enum ReactError {
    #[fail(display = "No cell with ID {:?} was found", id)]
    MissingCell { id: CellID},
    #[fail(display = "Expected input cell at ID {:?}, found computed cell", id)]
    ExpectedInputCell { id: CellID },
    #[fail(display = "Expected computed cell at ID {:?}, found input cell", id)]
    ExpectedComputedCell { id: CellID },
    #[fail(display = "Can't create computed cell as its missing the following dependencies: {:?}", missing_deps)]
    MissingDepedencies { missing_deps: Vec<CellID>},
    #[fail(display = "Can't delete a callback at ID {:?} as it doesn't exist", id)]
    CallbackDoesntExist { id: CallbackID },
}

impl <'a, T> Cell<'a, T> {
    // Gets the (cached) value for the given cell.
    pub fn value(&self) -> &T {
        match self {
            &Cell::Input(ref cell) => &cell.value,
            &Cell::Computed(ref cell) => &cell.value,
        }
    }
}


// You are guaranteed that Reactor will only be tested against types that are Copy + PartialEq.
impl <'a, T: Copy + PartialEq> Reactor<'a, T> {
    pub fn new() -> Self {
        Reactor {
            dep_graph: Graph::new(),
            cur_callback_id: 0,
        }
    }

    // Creates an input cell with the specified initial value, returning its ID.
    pub fn create_input(&mut self, initial: T) -> CellID {
        let input = InputCell { value: initial };
        self.dep_graph.add_node(Cell::Input(input))
    }

    // Creates a compute cell with the specified dependencies and compute function.
    // The compute function is expected to take in its arguments in the same order as specified in
    // `dependencies`.
    // You do not need to reject compute functions that expect more arguments than there are
    // dependencies (how would you check for this, anyway?).
    //
    // Return an Err (and you can change the error type) if any dependency doesn't exist.
    //
    // Notice that there is no way to *remove* a cell.
    // This means that you may assume, without checking, that if the dependencies exist at creation
    // time they will continue to exist as long as the Reactor exists.
    pub fn create_compute<F: 'static + Fn(&[T]) -> T>(&mut self, dependencies: &[CellID], compute_func: F) -> Result<CellID, ReactError> {
        let missing_deps = dependencies.iter()
            .cloned()
            .filter(|&dep| self.dep_graph.node_weight(dep).is_none())
            .collect::<Vec<_>>();
        if missing_deps.len() > 0 {
            return Err(ReactError::MissingDepedencies {
                missing_deps
            })
        }
        let dependant_values = dependencies.iter()
            .map(|&id| self.value(id).unwrap())
            .collect::<Vec<_>>();
        let value = compute_func(&dependant_values);
        let computed = ComputedCell {
            compute_func: Box::new(compute_func),
            callbacks: HashMap::new(),
            value
        };
        let node = self.dep_graph.add_node(Cell::Computed(computed));
        for (ix, &dep) in dependencies.iter().enumerate() {
            self.dep_graph.add_edge(node, dep, ix);
        }
        Ok(node)
    }

    // Retrieves the current value of the cell, or None if the cell does not exist.
    //
    // You may wonder whether it is possible to implement `get(&self, id: CellID) -> Option<&Cell>`
    // and have a `value(&self)` method on `Cell`.
    //
    // It turns out this introduces a significant amount of extra complexity to this exercise.
    // We chose not to cover this here, since this exercise is probably enough work as-is.
    pub fn value(&self, id: CellID) -> Option<T> {
        self.dep_graph.node_weight(id).map(|cell| *cell.value())
    }

    // Sets the value of the specified input cell.
    //
    // Return an Err (and you can change the error type) if the cell does not exist, or the
    // specified cell is a compute cell, since compute cells cannot have their values directly set.
    //
    // Similarly, you may wonder about `get_mut(&mut self, id: CellID) -> Option<&mut Cell>`, with
    // a `set_value(&mut self, new_value: T)` method on `Cell`.
    //
    // As before, that turned out to add too much extra complexity.
    pub fn set_value(&mut self, id: CellID, new_value: T) -> Result<(), ReactError> {
        self.dep_graph.node_weight_mut(id).ok_or(ReactError::MissingCell { id}).and_then(|cell| match *cell {
            Cell::Input(InputCell { ref mut value}) => {
                *value = new_value;
                Ok(())
            },
            Cell::Computed { .. } => Err(ReactError::ExpectedInputCell {
                id
            })
        })?;
        let affected_cells = self.find_deep_dependencies_on(id);
        let values_before_set = affected_cells.iter()
            .map(|&dep| (dep, self.value(dep).unwrap()))
            .collect::<HashMap<_,_>>();
        self.update_dependants(id)?;
        let values_after_set = affected_cells.iter()
            .map(|&dep| (dep, self.value(dep).unwrap()))
            .collect::<HashMap<_,_>>();
        values_after_set.into_iter()
            .filter(|&(node, new_value)| new_value != values_before_set[&node])
            .map(|(node, _)| self.invoke_callback(node))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }


    // Updates a computed cell's value by applying the computation function on its
    // dependencies, and also returns the updated value.
    // If given cell is an input cell, it'll always return its input value.
    fn compute_cell_shallow(&mut self, id: CellID) -> Result<T, ReactError> {
        let mut dependency_values = BTreeMap::new();
        let mut dependency_walker = self.dep_graph.neighbors_directed(id, Direction::Outgoing).detach();
        while let Some((edge, node)) = dependency_walker.next(&self.dep_graph) {
            let &ix = self.dep_graph.edge_weight(edge).unwrap();
            let &val = self.dep_graph.node_weight(node).unwrap().value();
            dependency_values.insert(ix, val);
        }
        let dependency_values = dependency_values.into_iter().map(|kvp| kvp.1).collect::<Vec<_>>();
        self.dep_graph.node_weight_mut(id).ok_or(ReactError::MissingCell { id}).and_then(|cell| match cell {
            &mut Cell::Input(InputCell { ref value }) => Ok(*value),
            &mut Cell::Computed(ComputedCell { ref mut value, ref compute_func, .. }) => {
                *value = compute_func(&dependency_values);
                Ok(*value)
            }
        })
    }

    // finds all computed cells that depend on the given cell, directly and indirectly.
    fn find_deep_dependencies_on(&self, id: CellID) -> HashSet<CellID> {
        let mut set = HashSet::new();
        let mut walker = self.dep_graph.neighbors_directed(id, Direction::Incoming).detach();
        while let Some(dep) = walker.next_node(&self.dep_graph) {
            set.insert(dep);
            set.extend(self.find_deep_dependencies_on(dep).iter());
        }
        set
    }

    // given a cell, recursively updates cells that depend on it
    pub fn update_dependants(&mut self, id: CellID) -> Result<(), ReactError> {
        // first, we update the cell itself
        let cell_value = self.compute_cell_shallow(id)?;
        // then, we update the cells that depend on it
        let mut depends_on_walker = self.dep_graph.neighbors_directed(id, Direction::Incoming).detach();
        while let Some(dep) = depends_on_walker.next_node(&self.dep_graph) {
            self.update_dependants(dep)?;
        }
        Ok(())
    }

    // Tries invoking the callbacks on a compute cell with the given ID.
    fn invoke_callback(&mut self, id: CellID) -> Result<(), ReactError> {
        self.dep_graph.node_weight_mut(id).ok_or(ReactError::MissingCell { id}).and_then(|val| match val {
            &mut Cell::Input(_) => Err(ReactError::ExpectedComputedCell { id }),
            &mut Cell::Computed(ComputedCell { ref value, ref mut callbacks, .. }) => {
                callbacks.values_mut().for_each(|cb| cb(*value));
                Ok(())
            }
        })
    }

    // Adds a callback to the specified compute cell.
    //
    // Return an Err (and you can change the error type) if the cell does not exist.
    //
    // Callbacks on input cells will not be tested.
    //
    // The semantics of callbacks (as will be tested):
    // For a single set_value call, each compute cell's callbacks should each be called:
    // * Zero times if the compute cell's value did not change as a result of the set_value call.
    // * Exactly once if the compute cell's value changed as a result of the set_value call.
    //   The value passed to the callback should be the final value of the compute cell after the
    //   set_value call.
    pub fn add_callback<F: FnMut(T) -> () + 'a>(&mut self, cell: CellID, callback: F) -> Result<CallbackID, ReactError> {
        let mut id = &mut self.cur_callback_id;
        self.dep_graph.node_weight_mut(cell).ok_or(ReactError::MissingCell { id: cell}).and_then(move |val| match *val {
            Cell::Input(_) => Err(ReactError::ExpectedComputedCell { id: cell}),
            Cell::Computed(ComputedCell { ref mut callbacks, ..}) => {
                let cb = Box::new(callback);
                callbacks.insert(*id, cb);
                *id += 1;
                Ok(*id - 1)
            }
        })
    }

    // Removes the specified callback, using an ID returned from add_callback.
    //
    // Return an Err (and you can change the error type) if either the cell or callback
    // does not exist.
    //
    // A removed callback should no longer be called.
    pub fn remove_callback(&mut self, cell: CellID, callback: CallbackID) -> Result<(), ReactError> {
        self.dep_graph.node_weight_mut(cell).ok_or(ReactError::MissingCell { id: cell}).and_then(|val| match *val {
            Cell::Input(_) => Err(ReactError::ExpectedComputedCell { id: cell}),
            Cell::Computed(ComputedCell { ref mut callbacks, ..}) => {
                if !callbacks.contains_key(&callback) {
                    Err(ReactError::CallbackDoesntExist { id: callback})
                } else {
                    callbacks.remove(&callback);
                    Ok(())
                }
            }
        })
    }
}
