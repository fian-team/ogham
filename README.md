# Ogham
> At least it's not JavaScript!

## What is Ogham?

Ogham is a language designed for UI development. It draws inspiration from the DOM for representing page structure, CSS for styling (flexbox in particular), and React for component structure, state management, and much more.

The objective in creating Ogham is to simplify UI development in Rust applications; although it ships with [Skia](https://skia.org/) rendering out of the box via [rust-skia](https://github.com/rust-skia/rust-skia), custom rendering backends can be implemented to allow integrations with projects where using Skia may have obstacles. State can be tracked within a given Ogham application or provided by the host Rust application; events may be propagated upwards from the Ogham app to its host application.

Alternatively, Ogham can be used to create standalone applications using the Ogham browser. This functionality is somewhat useless in practice, as many features necessary to create applications on parity with modern web apps are missing (the ability to make network requests comes to mind). Maybe it will be viable someday, who knows!

## Basic Example

Here's a simple counter app that tracks state and updates on button clicks:

```ogh
let counter = () widget {
  state count = 0;
  
  Flex {
    children: [
      Button {
        text: "Increment",
        on_click: () {
          count++;
        },
      },
      Text {
        text: count -> string,
      },
    ],
  }
};

let main = () {
  counter()
};
```

We can see a number of concepts displayed here.

- Defining variables with the `let` keyword.
- Creating components by defining a function with the `widget` return type.
- Implicitly returning values from functions following the Rust idiom of omitting semicolons.
- Creating an application entry point by defining a `main` function.
- Calling (and implicitly returning the value of) `counter` in the `main` function.

## Getting Started

### Standalone

Create an `.ogh` file with your UI code and open it in the Ogham browser. The browser provides a development environment for writing and testing Ogham applications.

[Download Ogham Browser]() *(Releases coming soon)*

### Integration

Coming soon!

## Contributing

We welcome contributions! Flag bugs, submit pull requests, and join our developer community. Have questions or want to discuss development? Join us on Discord.

[Join the Fian Dev Community](https://discord.gg/JYfC2baP2y)

