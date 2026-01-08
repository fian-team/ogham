pub const HOME_PAGE: &str = r#"

let main = fn () {
  Flex {
    width: "grow",
    height: "grow",
    main_alignment: "center",
    cross_alignment: "center",
    background_color: {
      r: 30,
      g: 30,
      b: 30,
      a: 255,
    },
    padding: 16,
    children: [
      Flex {
        width: 240,
        height: "shrink",
        direction: "column",
        gap: 16,
        padding: 16,
        background_color: {
          r: 60,
          g: 60,
          b: 60,
          a: 255,
        },
        corner_radius: 8,
        border: {
          width: 2,
          color: {
            r: 100,
            g: 100,
            b: 100,
            a: 255,
          },
          style: "solid",
        },
        children: [
          Flex {
            width: "grow",
            height: 32,
            children: [
              Text {
                color: {
                  r: 255,
                  g: 255,
                  b: 255,
                  a: 255,
                },
                align: "center",
                text: "Welcome to Ogham.",
              }
            ],
          },
          Flex {
            width: "grow",
            height: 32,
            children: [
              Text {
                color: {
                  r: 255,
                  g: 255,
                  b: 255,
                  a: 255,
                },
                align: "center",
                text: "Press ctrl+o to open a file.",
              }
            ],
          },
        ],
      },
    ]
  }
};

"#;
