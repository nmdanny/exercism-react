#[allow(unused_variables)]

use std::collections::HashMap;

// Because these are passed without & to some functions,
// it will probably be necessary for these two types to be Copy.
pub type CellID = usize;
pub type CallbackID = usize;

pub struct Reactor<'a, T> {
    // Just so that the compiler doesn't complain about an unused type parameter.
    // You probably want to delete this field.
    values: HashMap<CellID, Value<'a, T>>,
    cur_cell_id: usize,
    cur_callback_id: usize
}


enum Value<'a, T> {
    Input(T),
    // todo: cache computed values
    Computed {
        dependencies: Vec<CellID>,
        compute_func: Box<Fn(&[T]) -> T>,
        callbacks: HashMap<CallbackID, Box<FnMut(T) -> () + 'a>>,
    }
}

enum Error {
    MissingDependencies { missing: Vec<CellID> },
    NoSuchCellExists { id: CellID }
}

// You are guaranteed that Reactor will only be tested against types that are Copy + PartialEq.
impl <'a, T: Copy + PartialEq> Reactor<'a, T> {
    pub fn new() -> Self {
        Reactor {
            values: HashMap::new(),
            cur_cell_id: 0,
            cur_callback_id: 0
        }
    }

    // Creates an input cell with the specified initial value, returning its ID.
    pub fn create_input(&mut self, initial: T) -> CellID {
        let input = Value::Input(initial);
        let id = self.cur_cell_id;
        self.values.insert(id, input);
        self.cur_cell_id += 1;
        id
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
    pub fn create_compute<F: 'static + Fn(&[T]) -> T>(&mut self, dependencies: &[CellID], compute_func: F) -> Result<CellID, ()> {
        if !dependencies.iter().all(|dep| self.values.contains_key(dep)) {
            return Err(());
        }
        let input = Value::Computed {
            dependencies: dependencies.to_vec(),
            compute_func: Box::new(compute_func),
            callbacks: HashMap::new()
        };
        let id = self.cur_cell_id;
        self.values.insert(id, input);
        self.cur_cell_id += 1;
        Ok(id)
    }

    // Retrieves the current value of the cell, or None if the cell does not exist.
    //
    // You may wonder whether it is possible to implement `get(&self, id: CellID) -> Option<&Cell>`
    // and have a `value(&self)` method on `Cell`.
    //
    // It turns out this introduces a significant amount of extra complexity to this exercise.
    // We chose not to cover this here, since this exercise is probably enough work as-is.
    pub fn value(&self, id: CellID) -> Option<T> {
        self.values.get(&id).map(|val| match val {
            &Value::Input(t) => t,
            &Value::Computed { ref dependencies, ref compute_func, .. } => {
                let dep_values = dependencies.iter()
                    .filter_map(|&dep| self.value(dep))
                    .collect::<Vec<_>>();
                compute_func(&dep_values)
            }
        })
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
    pub fn set_value(&mut self, id: CellID, new_value: T) -> Result<(), ()> {
        self.values.get_mut(&id).ok_or(()).and_then(|val| match *val {
            Value::Input(ref mut t) => {
                *t = new_value;
                Ok(())
            },
            Value::Computed { .. } => Err(())
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
    pub fn add_callback<F: FnMut(T) -> () + 'a>(&mut self, id: CellID, callback: F) -> Result<CallbackID, ()> {
        let mut id = &mut self.cur_callback_id;
        self.values.get_mut(&id).ok_or(()).and_then(move |val| match *val {
            Value::Input(_) => Err(()),
            Value::Computed { ref mut callbacks, ..} => {
                let cb = Box::new(callback);
                callbacks.insert(*id, cb);
                *id += 1;
                Ok(*id)
            }
        })
    }

    // Removes the specified callback, using an ID returned from add_callback.
    //
    // Return an Err (and you can change the error type) if either the cell or callback
    // does not exist.
    //
    // A removed callback should no longer be called.
    pub fn remove_callback(&mut self, cell: CellID, callback: CallbackID) -> Result<(), ()> {
        self.values.get_mut(&cell).ok_or(()).and_then(|val| match *val {
            Value::Input(_) => Err(()),
            Value::Computed { ref mut callbacks, ..} => {
                callbacks.remove(&callback);
                Ok(())
            }
        })
    }
}
