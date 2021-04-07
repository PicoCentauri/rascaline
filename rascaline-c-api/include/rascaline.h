/* ============    Automatically generated file, DOT NOT EDIT.    ============ *
 *                                                                             *
 *    This file is automatically generated from the rascaline-c-api sources,   *
 *    using cbindgen. If you want to make change to this file (including       *
 *    documentation), make the corresponding changes in the rust sources.      *
 * =========================================================================== */

#ifndef RASCALINE_H
#define RASCALINE_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * The different kinds of indexes that can exist on a `rascal_descriptor_t`
 */
typedef enum rascal_indexes {
  /**
   * The feature index, describing the features of the representation
   */
  RASCAL_INDEXES_FEATURES = 0,
  /**
   * The samples index, describing different samples in the representation
   */
  RASCAL_INDEXES_SAMPLES = 1,
  /**
   * The gradient samples index, describing the gradients of samples in the
   * representation with respect to other atoms
   */
  RASCAL_INDEXES_GRADIENT_SAMPLES = 2,
} rascal_indexes;

/**
 * Status type returned by all functions in the C API.
 */
typedef enum rascal_status_t {
  /**
   * The function succeeded
   */
  RASCAL_SUCCESS = 0,
  /**
   * A function got an invalid parameter
   */
  RASCAL_INVALID_PARAMETER_ERROR = 1,
  /**
   * There was an error reading or writing JSON
   */
  RASCAL_JSON_ERROR = 2,
  /**
   * A string contains non-utf8 data
   */
  RASCAL_UTF8_ERROR = 3,
  /**
   * There was an error of unknown kind
   */
  RASCAL_UNKNOWN_ERROR = 254,
  /**
   * There was an internal error (rust panic)
   */
  RASCAL_INTERNAL_PANIC = 255,
} rascal_status_t;

/**
 * Opaque type representing a `Calculator`
 */
typedef struct rascal_calculator_t rascal_calculator_t;

/**
 * Opaque type representing a `Descriptor`.
 */
typedef struct rascal_descriptor_t rascal_descriptor_t;

/**
 * Pair of atoms coming from a neighbor list
 */
typedef struct rascal_pair_t {
  /**
   * index of the first atom in the pair
   */
  uintptr_t first;
  /**
   * index of the second atom in the pair
   */
  uintptr_t second;
  /**
   * vector from the first atom to the second atom, wrapped inside the unit
   * cell as required by periodic boundary conditions.
   */
  double vector[3];
} rascal_pair_t;

/**
 * A `rascal_system_t` deals with the storage of atoms and related information,
 * as well as the computation of neighbor lists.
 *
 * This struct contains a manual implementation of a virtual table, allowing to
 * implement the rust `System` trait in C and other languages. Speaking in Rust
 * terms, `user_data` contains a pointer (analog to `Box<Self>`) to the struct
 * implementing the `System` trait; and then there is one function pointers
 * (`Option<unsafe extern fn(XXX)>`) for each function in the `System` trait.
 *
 * A new implementation of the System trait can then be created in any language
 * supporting a C API (meaning any language for our purposes); by correctly
 * setting `user_data` to the actual data storage, and setting all function
 * pointers to the correct functions. For an example of code doing this, see
 * the `SystemBase` class in the Python interface to rascaline.
 */
typedef struct rascal_system_t {
  /**
   * User-provided data should be stored here, it will be passed as the
   * first parameter to all function pointers below.
   */
  void *user_data;
  /**
   * This function should set `*size` to the number of atoms in this system
   */
  void (*size)(const void *user_data, uintptr_t *size);
  /**
   * This function should set `*species` to a pointer to the first element of
   * a contiguous array containing the atomic species of each atom in the
   * system. Different atomic species should be identified with a different
   * value. These values are usually the atomic number, but don't have to be.
   * The array should contain `rascal_system_t::size()` elements.
   */
  void (*species)(const void *user_data, const uintptr_t **species);
  /**
   * This function should set `*positions` to a pointer to the first element
   * of a contiguous array containing the atomic cartesian coordinates.
   * `positions[0], positions[1], positions[2]` must contain the x, y, z
   * cartesian coordinates of the first atom, and so on.
   */
  void (*positions)(const void *user_data, const double **positions);
  /**
   * This function should write the unit cell matrix in `cell`, which have
   * space for 9 values.
   */
  void (*cell)(const void *user_data, double *cell);
  /**
   * This function should compute the neighbor list with the given cutoff,
   * and store it for later access using `pairs` or `pairs_containing`.
   */
  void (*compute_neighbors)(void *user_data, double cutoff);
  /**
   * This function should set `*pairs` to a pointer to the first element of a
   * contiguous array containing all pairs in this system; and `*count` to
   * the size of the array/the number of pairs.
   *
   * This list of pair should only contain each pair once (and not twice as
   * `i-j` and `j-i`), should not contain self pairs (`i-i`); and should only
   * contains pairs where the distance between atoms is actually bellow the
   * cutoff passed in the last call to `compute_neighbors`. This function is
   * only valid to call after a call to `compute_neighbors`.
   */
  void (*pairs)(const void *user_data, const struct rascal_pair_t **pairs, uintptr_t *count);
  /**
   * This function should set `*pairs` to a pointer to the first element of a
   * contiguous array containing all pairs in this system containing the atom
   * with index `center`; and `*count` to the size of the array/the number of
   * pairs.
   *
   * The same restrictions on the list of pairs as `rascal_system_t::pairs`
   * applies, with the additional condition that the pair `i-j` should be
   * included both in the return of `pairs_containing(i)` and
   * `pairs_containing(j)`.
   */
  void (*pairs_containing)(const void *user_data, uintptr_t center, const struct rascal_pair_t **pairs, uintptr_t *count);
} rascal_system_t;

/**
 * Options that can be set to change how a calculator operates.
 */
typedef struct rascal_calculation_options_t {
  /**
   * Copy the data from systems into native `SimpleSystem`. This can be
   * faster than having to cross the FFI boundary too often.
   */
  bool use_native_system;
  /**
   * List of samples on which to run the calculation. Use `NULL` to run the
   * calculation on all samples. The samples must be represented as a
   * row-major array, containing values similar to the samples index of a
   * descriptor. If necessary, gradients samples will be derived from the
   * values given in selected_samples.
   */
  const int32_t *selected_samples;
  /**
   * If selected_samples is not `NULL`, this should be set to the size of the
   * selected_samples array
   */
  uintptr_t selected_samples_count;
  /**
   * List of features on which to run the calculation. Use `NULL` to run the
   * calculation on all features. The features must be represented as a
   * row-major array, containing values similar to the features index of a
   * descriptor.
   */
  const int32_t *selected_features;
  /**
   * If selected_features is not `NULL`, this should be set to the size of the
   * selected_features array
   */
  uintptr_t selected_features_count;
} rascal_calculation_options_t;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Get the last error message that was created on the current thread.
 *
 * @returns the last error message, as a NULL-terminated string
 */
const char *rascal_last_error(void);

/**
 * Read all structures in the file at the given `path` using
 * [chemfiles](https://chemfiles.org/), and convert them to an array of
 * `rascal_system_t`.
 *
 * This function can read all [formats supported by
 * chemfiles](https://chemfiles.org/chemfiles/latest/formats.html).
 *
 * This function allocates memory, which must be released using
 * `rascal_basic_systems_free`.
 *
 * If you need more control over the system behavior, consider writing your own
 * instance of `rascal_system_t`.
 *
 * @param path path of the file to read from in the local filesystem
 * @param systems `*systems` will be set to a pointer to the first element of
 *                 the array of `rascal_system_t`
 * @param count `*count` will be set to the number of systems read from the file
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_basic_systems_read(const char *path,
                                               struct rascal_system_t **systems,
                                               uintptr_t *count);

/**
 * Release memory allocated by `rascal_basic_systems_read`.
 *
 * This function is only valid to call with a pointer to systems obtained from
 * `rascal_basic_systems_read`, and the corresponding `count`. Any other use
 * will probably result in segmentation faults or double free. If `systems` is
 * NULL, this function does nothing.
 *
 * @param systems pointer to the first element of the array of
 * `rascal_system_t` @param count number of systems in the array
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_basic_systems_free(struct rascal_system_t *systems, uintptr_t count);

/**
 * Create a new empty descriptor.
 *
 * All memory allocated by this function can be released using
 * `rascal_descriptor_free`.
 *
 * @returns A pointer to the newly allocated descriptor, or a `NULL` pointer in
 *          case of error. In case of error, you can use `rascal_last_error()`
 *          to get the error message.
 */
struct rascal_descriptor_t *rascal_descriptor(void);

/**
 * Free the memory associated with a `descriptor` previously created with
 * `rascal_descriptor`.
 *
 * If `descriptor` is `NULL`, this function does nothing.
 *
 * @param descriptor pointer to an existing descriptor, or `NULL`
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the
 *          full error message.
 */
enum rascal_status_t rascal_descriptor_free(struct rascal_descriptor_t *descriptor);

/**
 * Get the values stored inside this descriptor after a call to
 * `rascal_calculator_compute`.
 *
 * This function sets `*data` to a **read only** pointer containing the address
 * of first element of the 2D array containing the values, `*samples` to the
 * size of the first axis of this array and `*features` to the size of the
 * second axis of the array. The array is stored using a row-major layout.
 *
 * @param descriptor pointer to an existing descriptor
 * @param data pointer to a pointer to a double, will be set to the address of
 *             the first element in the values array
 * @param samples pointer to a single integer, will be set to the first
 *                dimension of the values array
 * @param features pointer to a single integer, will be set to the second
 *                 dimension of the values array
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_descriptor_values(const struct rascal_descriptor_t *descriptor,
                                              const double **data,
                                              uintptr_t *samples,
                                              uintptr_t *features);

/**
 * Get the gradients stored inside this descriptor after a call to
 * `rascal_calculator_compute`, if any.
 *
 * This function sets `*data` to to a **read only** pointer containing the
 * address of the first element of the 2D array containing the gradients,
 * `*gradient_samples` to the size of the first axis of this array and
 * `*features` to the size of the second axis of the array. The array is stored
 * using a row-major layout.
 *
 * If this descriptor does not contain gradient data, `*data` is set to `NULL`,
 * while `*gradient_samples` and `*features` are set to 0.
 *
 * @param descriptor pointer to an existing descriptor
 * @param data pointer to a pointer to a double, will be set to the address of
 *             the first element in the gradients array
 * @param gradient_samples pointer to a single integer, will be set to the first
 *                         dimension of the gradients array
 * @param features pointer to a single integer, will be set to the second
 *                 dimension of the gradients array
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_descriptor_gradients(const struct rascal_descriptor_t *descriptor,
                                                 const double **data,
                                                 uintptr_t *gradient_samples,
                                                 uintptr_t *features);

/**
 * Get the values associated with one of the `indexes` in the given
 * `descriptor`.
 *
 * This function sets `*data` to to a **read only** pointer containing the
 * address of the first element of the 2D array containing the index values,
 * `*count` to the number of indexes (first dimension of the array) and `*size`
 * to the size of each index (second dimension of the array). The array is
 * stored using a row-major layout.
 *
 * If this `descriptor` does not contain gradient data, and `indexes` is
 * `RASCAL_INDEXES_GRADIENTS`, `*data` is set to `NULL`, while
 * `*count` and `*size` are set to 0.
 *
 * @param descriptor pointer to an existing descriptor
 * @param indexes type of indexes requested
 * @param data pointer to a pointer to a double, will be set to the address of
 *             the first element in the index array
 * @param count pointer to a single integer, will be set to the number of
 *              index values
 * @param size pointer to a single integer, will be set to the size of each
 *              index value
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_descriptor_indexes(const struct rascal_descriptor_t *descriptor,
                                               enum rascal_indexes indexes,
                                               const int32_t **data,
                                               uintptr_t *count,
                                               uintptr_t *size);

/**
 * Get the names associated with one of the `indexes` in the given
 * `descriptor`.
 *
 * If this `descriptor` does not contain gradient data, and `indexes` is
 * `RASCAL_INDEXES_GRADIENTS`, each pointer in `*names` is set to `NULL`.
 *
 * The `size` value should correspond to the value set by
 * `rascal_descriptor_indexes` in the `size` parameter.
 *
 * @param descriptor pointer to an existing descriptor
 * @param indexes type of indexes requested
 * @param names pointer to the first element of an array of `const char*`
 *              that will be filled with **read only** pointers to the index
 *              names
 * @param size size of the `names` array, i.e. number of elements inside
 *             the array
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_descriptor_indexes_names(const struct rascal_descriptor_t *descriptor,
                                                     enum rascal_indexes indexes,
                                                     const char **names,
                                                     uintptr_t size);

enum rascal_status_t rascal_descriptor_densify(struct rascal_descriptor_t *descriptor,
                                               const char *const *variables,
                                               uintptr_t count);

/**
 * Create a new calculator with the given `name` and `parameters`.
 *
 * @verbatim embed:rst:leading-asterisk
 *
 * The list of available calculators and the corresponding parameters are in
 * the :ref:`main documentation <calculators-list>`. The ``parameters`` should
 * be formatted as JSON, according to the requested calculator schema.
 *
 * @endverbatim
 *
 * All memory allocated by this function can be released using
 * `rascal_calculator_free`.
 *
 * @param name name of the calculator as a NULL-terminated string
 * @param parameters hyper-parameters of the calculator, JSON-formatted in a
 *                   NULL-terminated string
 *
 * @returns A pointer to the newly allocated calculator, or a `NULL` pointer in
 *          case of error. In case of error, you can use `rascal_last_error()`
 *          to get the error message.
 */
struct rascal_calculator_t *rascal_calculator(const char *name, const char *parameters);

/**
 * Free the memory associated with a `calculator` previously created with
 * `rascal_calculator`.
 *
 * If `calculator` is `NULL`, this function does nothing.
 *
 * @param calculator pointer to an existing calculator, or `NULL`
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the
 *          full error message.
 */
enum rascal_status_t rascal_calculator_free(struct rascal_calculator_t *calculator);

/**
 * Get a copy of the name of this calculator in the `name` buffer of size
 * `bufflen`.
 *
 *`name` will be NULL-terminated by this function. If the buffer is too small
 * to fit the whole name, this function will return
 * `RASCAL_INVALID_PARAMETER_ERROR`
 *
 * @param calculator pointer to an existing calculator
 * @param name string buffer to fill with the calculator name
 * @param bufflen number of characters available in the buffer
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_calculator_name(const struct rascal_calculator_t *calculator,
                                            char *name,
                                            uintptr_t bufflen);

/**
 * Get a copy of the parameters used to create this calculator in the
 * `parameters` buffer of size `bufflen`.
 *
 * `parameters` will be NULL-terminated by this function. If the buffer is too
 * small to fit the whole name, this function will return
 * `RASCAL_INVALID_PARAMETER_ERROR`.
 *
 * @param calculator pointer to an existing calculator
 * @param parameters string buffer to fill with the parameters used to create
 *                   this calculator
 * @param bufflen number of characters available in the buffer
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_calculator_parameters(const struct rascal_calculator_t *calculator,
                                                  char *parameters,
                                                  uintptr_t bufflen);

/**
 * Run a calculation with the given `calculator` on the given `systems`,
 * storing the resulting data in the `descriptor`.
 *
 * @param calculator pointer to an existing calculator
 * @param descriptor pointer to an existing descriptor for data storage
 * @param systems pointer to an array of systems implementation
 * @param systems_count number of systems in `systems`
 * @param options options for this calculation
 *
 * @returns The status code of this operation. If the status is not
 *          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
 *          error message.
 */
enum rascal_status_t rascal_calculator_compute(struct rascal_calculator_t *calculator,
                                               struct rascal_descriptor_t *descriptor,
                                               struct rascal_system_t *systems,
                                               uintptr_t systems_count,
                                               struct rascal_calculation_options_t options);

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus

#endif /* RASCALINE_H */